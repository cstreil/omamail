//! Read-only JMAP Contacts projection: recipient suggestions and address books.
use super::{Session, mailbox};
use serde_json::{Value, json};
use std::{collections::HashSet, sync::Arc, time::Duration};

const CORE: &str = "urn:ietf:params:jmap:core";
const CONTACTS: &str = "urn:ietf:params:jmap:contacts";
const CALL_TIME: Duration = Duration::from_secs(25);
const MAX_REMOTE_CONTACTS: usize = 1000;
const MAX_LIST: usize = 2000;
const MAX_BOOKS: usize = 256;
const MAX_EMAILS_PER_CARD: usize = 64;
const MAX_ENTRIES_PER_CARD: usize = 64;
const MAX_COMPONENTS: usize = 64;
const MAX_TEXT: usize = 4096;
const MAX_LABEL: usize = 64;
const CARD_PROPERTIES: &[&str] = &["id", "addressBookIds", "name", "emails"];
const DETAIL_PROPERTIES: &[&str] = &[
    "id",
    "addressBookIds",
    "name",
    "emails",
    "phones",
    "addresses",
];

impl Session {
    pub(crate) async fn contact_suggestions(
        &self,
        account_id: &str,
    ) -> Result<Value, &'static str> {
        let (context, snapshot, account) = self.contacts_setup(account_id).await?;
        let result = tokio::time::timeout(CALL_TIME, async {
            let books = self.books(&context, &snapshot, &account).await?;
            let source = choose_source(&books)?;
            let (cards, missing) = self
                .cards_for(
                    &context,
                    &snapshot,
                    &account,
                    &source,
                    MAX_REMOTE_CONTACTS,
                    0,
                )
                .await?;
            if missing > 0 {
                return Err("jmap_invalid_response");
            }
            normalize(&cards)
        })
        .await
        .unwrap_or(Err("jmap_timeout"));
        note(&context, &result);
        result
    }

    pub(crate) async fn contact_sources(&self, account_id: &str) -> Result<Value, &'static str> {
        let (context, snapshot, account) = self.contacts_setup(account_id).await?;
        let result = tokio::time::timeout(CALL_TIME, async {
            let books = self.books(&context, &snapshot, &account).await?;
            sources_of(&books)
        })
        .await
        .unwrap_or(Err("jmap_timeout"));
        note(&context, &result);
        result
    }

    pub(crate) async fn contact_list(
        &self,
        account_id: &str,
        source: &str,
        query: &str,
        limit: usize,
        position: usize,
    ) -> Result<Value, &'static str> {
        let (context, snapshot, account) = self.contacts_setup(account_id).await?;
        let result = tokio::time::timeout(CALL_TIME, async {
            let books = self.books(&context, &snapshot, &account).await?;
            if !readable_book(&books, source)? {
                return Err("contacts_source_unknown");
            }
            let mut filter = json!({"inAddressBook": source});
            if !query.is_empty() {
                filter["text"] = json!(query);
            }
            let ids = self
                .card_ids(&context, &snapshot, &account, filter, MAX_LIST)
                .await?;
            let (cards, missing) = self
                .cards(&context, &snapshot, &account, &ids, CARD_PROPERTIES)
                .await?;
            if missing > 0 {
                return Err("jmap_invalid_response");
            }
            let mut rows = cards
                .iter()
                .map(|card| list_row(card, source))
                .collect::<Result<Vec<_>, &'static str>>()?;
            rows.sort_by(|a, b| sort_key(a).cmp(&sort_key(b)));
            let total = rows.len();
            let start = position.min(total);
            let end = start.saturating_add(limit).min(total);
            let contacts: Vec<Value> = rows.drain(..).skip(start).take(end - start).collect();
            Ok(json!({"contacts": contacts, "total": total, "position": start}))
        })
        .await
        .unwrap_or(Err("jmap_timeout"));
        note(&context, &result);
        result
    }

    pub(crate) async fn contact_detail(
        &self,
        account_id: &str,
        id: &str,
    ) -> Result<Value, &'static str> {
        let (context, snapshot, account) = self.contacts_setup(account_id).await?;
        let ids = vec![bounded_id(id)?.to_owned()];
        let result = tokio::time::timeout(CALL_TIME, async {
            let (cards, missing) = self
                .cards(&context, &snapshot, &account, &ids, DETAIL_PROPERTIES)
                .await?;
            if missing > 0 {
                return Err("contacts_not_found");
            }
            let card = cards.first().ok_or("contacts_not_found")?;
            Ok(json!({"contact": contact_of(card)?}))
        })
        .await
        .unwrap_or(Err("jmap_timeout"));
        note(&context, &result);
        result
    }

    /// Session, capability and contact-account resolution shared by every call.
    async fn contacts_setup(
        &self,
        account_id: &str,
    ) -> Result<(Arc<mailbox::Context>, mailbox::Snapshot, String), &'static str> {
        let context = self.context(account_id)?;
        let setup = tokio::time::timeout(CALL_TIME, async {
            let snapshot = self.snapshot(account_id, &context).await?;
            let account = contacts_account(&snapshot)?.to_owned();
            Ok::<_, &'static str>((snapshot, account))
        })
        .await
        .unwrap_or(Err("jmap_timeout"));
        match setup {
            Ok((snapshot, account)) => Ok((context, snapshot, account)),
            Err(error) => {
                note(&context, &Err::<(), _>(error));
                Err(error)
            }
        }
    }

    async fn books(
        &self,
        context: &mailbox::Context,
        snapshot: &mailbox::Snapshot,
        account: &str,
    ) -> Result<Vec<Value>, &'static str> {
        let result = self
            .api_using(
                context,
                snapshot,
                json!([["AddressBook/get", {
                    "accountId": account,
                    "ids": null,
                    "properties": ["id", "name", "isDefault", "isSubscribed", "myRights"]
                }, "contacts-books"]]),
                json!([CORE, CONTACTS]),
            )
            .await?;
        let books = mailbox::argument(&result, "contacts-books", "AddressBook/get")?["list"]
            .as_array()
            .ok_or("jmap_invalid_response")?;
        if books.len() > MAX_BOOKS {
            return Err("jmap_response_too_large");
        }
        Ok(books.clone())
    }

    async fn card_ids(
        &self,
        context: &mailbox::Context,
        snapshot: &mailbox::Snapshot,
        account: &str,
        filter: Value,
        limit: usize,
    ) -> Result<Vec<String>, &'static str> {
        let result = self
            .api_using(
                context,
                snapshot,
                json!([["ContactCard/query", {
                    "accountId": account,
                    "filter": filter,
                    "position": 0,
                    "limit": limit,
                    "calculateTotal": true
                }, "contacts-query"]]),
                json!([CORE, CONTACTS]),
            )
            .await?;
        let query = mailbox::argument(&result, "contacts-query", "ContactCard/query")?;
        let raw_ids = query["ids"].as_array().ok_or("jmap_invalid_response")?;
        if raw_ids.len() > limit
            || query["total"]
                .as_u64()
                .is_some_and(|total| total > limit as u64)
        {
            return Err("contacts_too_many");
        }
        let mut seen = HashSet::new();
        let mut ids = Vec::with_capacity(raw_ids.len());
        for value in raw_ids {
            let id = bounded_id(value.as_str().ok_or("jmap_invalid_response")?)?;
            if !seen.insert(id.to_owned()) {
                return Err("jmap_invalid_response");
            }
            ids.push(id.to_owned());
        }
        Ok(ids)
    }

    async fn cards_for(
        &self,
        context: &mailbox::Context,
        snapshot: &mailbox::Snapshot,
        account: &str,
        source: &str,
        limit: usize,
        position: usize,
    ) -> Result<(Vec<Value>, usize), &'static str> {
        let result = self
            .api_using(
                context,
                snapshot,
                json!([["ContactCard/query", {
                    "accountId": account,
                    "filter": {"inAddressBook": source},
                    "position": position,
                    "limit": limit
                }, "contacts-query"]]),
                json!([CORE, CONTACTS]),
            )
            .await?;
        let query = mailbox::argument(&result, "contacts-query", "ContactCard/query")?;
        let raw_ids = query["ids"].as_array().ok_or("jmap_invalid_response")?;
        if raw_ids.len() > limit {
            return Err("jmap_response_too_large");
        }
        let mut seen = HashSet::new();
        let mut ids = Vec::with_capacity(raw_ids.len());
        for value in raw_ids {
            let id = bounded_id(value.as_str().ok_or("jmap_invalid_response")?)?;
            if !seen.insert(id.to_owned()) {
                return Err("jmap_invalid_response");
            }
            ids.push(id.to_owned());
        }
        self.cards(context, snapshot, account, &ids, CARD_PROPERTIES)
            .await
    }

    /// Fetches cards and reports how many requested ids the server did not
    /// return: a query result must be complete, a direct get may be absent.
    async fn cards(
        &self,
        context: &mailbox::Context,
        snapshot: &mailbox::Snapshot,
        account: &str,
        ids: &[String],
        properties: &[&str],
    ) -> Result<(Vec<Value>, usize), &'static str> {
        let mut cards = Vec::with_capacity(ids.len());
        let mut missing = 0usize;
        for (index, chunk) in ids
            .chunks(snapshot.limit("maxObjectsInGet", 256))
            .enumerate()
        {
            let tag = format!("contacts-get-{index}");
            let result = self
                .api_using(
                    context,
                    snapshot,
                    json!([["ContactCard/get", {
                        "accountId": account,
                        "ids": chunk,
                        "properties": properties
                    }, tag]]),
                    json!([CORE, CONTACTS]),
                )
                .await?;
            let list = mailbox::argument(&result, &tag, "ContactCard/get")?["list"]
                .as_array()
                .ok_or("jmap_invalid_response")?;
            let expected: HashSet<_> = chunk.iter().map(String::as_str).collect();
            let mut returned = HashSet::new();
            for card in list {
                let id = bounded_id(card["id"].as_str().ok_or("jmap_invalid_response")?)?;
                if !expected.contains(id) || !returned.insert(id.to_owned()) {
                    return Err("jmap_invalid_response");
                }
                cards.push(card.clone());
            }
            missing += expected.len() - returned.len();
        }
        Ok((cards, missing))
    }
}

fn note<T>(context: &mailbox::Context, result: &Result<T, &'static str>) {
    if matches!(result, Err("jmap_unauthorized")) {
        context
            .rejected
            .store(true, std::sync::atomic::Ordering::Release);
    }
}

fn contacts_account(snapshot: &mailbox::Snapshot) -> Result<&str, &'static str> {
    if !snapshot.document["capabilities"][CONTACTS].is_object() {
        return Err("contacts_not_supported");
    }
    let account = snapshot.document["primaryAccounts"][CONTACTS]
        .as_str()
        .ok_or("contacts_not_supported")?;
    bounded_id(account)?;
    if !snapshot.document["accounts"][account]["accountCapabilities"][CONTACTS].is_object() {
        return Err("contacts_not_supported");
    }
    Ok(account)
}

fn readable(book: &Value) -> bool {
    book["myRights"]["mayRead"] == true
}

fn choose_source(books: &[Value]) -> Result<String, &'static str> {
    if books.len() > MAX_BOOKS {
        return Err("jmap_response_too_large");
    }
    let selected = books
        .iter()
        .filter(|book| readable(book))
        .find(|book| book["isDefault"] == true)
        .or_else(|| {
            books
                .iter()
                .filter(|book| readable(book))
                .find(|book| book["isSubscribed"] == true)
        })
        .ok_or("contacts_no_address_book")?;
    Ok(bounded_id(selected["id"].as_str().ok_or("jmap_invalid_response")?)?.to_owned())
}

fn readable_book(books: &[Value], source: &str) -> Result<bool, &'static str> {
    if books.len() > MAX_BOOKS {
        return Err("jmap_response_too_large");
    }
    for book in books {
        let id = bounded_id(book["id"].as_str().ok_or("jmap_invalid_response")?)?;
        if id == source {
            return Ok(readable(book));
        }
    }
    Ok(false)
}

fn sources_of(books: &[Value]) -> Result<Value, &'static str> {
    let mut sources = Vec::with_capacity(books.len());
    for book in books {
        let rights = &book["myRights"];
        sources.push(json!({
            "id": bounded_id(book["id"].as_str().ok_or("jmap_invalid_response")?)?,
            "name": clean(book["name"].as_str().unwrap_or(""), MAX_TEXT),
            "default": book["isDefault"] == true,
            "subscribed": book["isSubscribed"] == true,
            "readOnly": !(rights["mayWrite"] == true),
        }));
    }
    Ok(json!({"sources": sources}))
}

fn clean(value: &str, limit: usize) -> String {
    value
        .chars()
        .filter(|character| !character.is_control())
        .take(limit)
        .collect()
}

fn display_name(name: &Value) -> String {
    let full = clean(name["full"].as_str().unwrap_or(""), MAX_TEXT);
    if !full.is_empty() {
        return full;
    }
    let mut parts = Vec::new();
    for kind in ["given", "middle", "surname"] {
        if let Some(components) = name["components"].as_array() {
            for component in components.iter().take(MAX_COMPONENTS) {
                if component["kind"] == kind
                    && let Some(value) = component["value"].as_str()
                {
                    let value = clean(value, MAX_TEXT);
                    if !value.is_empty() {
                        parts.push(value);
                    }
                    break;
                }
            }
        }
    }
    parts.join(" ")
}

fn label_of(entry: &Value) -> String {
    if let Some(label) = entry["label"].as_str().filter(|value| !value.is_empty()) {
        return clean(label, MAX_LABEL);
    }
    if let Some(contexts) = entry["contexts"].as_object() {
        for key in ["work", "private", "home", "other"] {
            if contexts.contains_key(key) {
                return key.to_owned();
            }
        }
        if let Some(key) = contexts.keys().next() {
            return clean(key, MAX_LABEL);
        }
    }
    String::new()
}

/// Labelled `{value,label}` rows for emails and telephone numbers.
fn labelled(entries: &Value, key: &str) -> Result<Value, &'static str> {
    let mut rows = Vec::new();
    let Some(map) = entries.as_object() else {
        return Ok(json!(rows));
    };
    if map.len() > MAX_ENTRIES_PER_CARD {
        return Err("jmap_response_too_large");
    }
    for entry in map.values() {
        let Some(value) = entry[key].as_str() else {
            continue;
        };
        let value = clean(value, MAX_TEXT);
        if value.is_empty() {
            continue;
        }
        rows.push(json!({"value": value, "label": label_of(entry)}));
    }
    Ok(json!(rows))
}

/// Postal addresses flatten their JSContact components into one display line.
fn postal(entries: &Value) -> Result<Value, &'static str> {
    let mut rows = Vec::new();
    let Some(map) = entries.as_object() else {
        return Ok(json!(rows));
    };
    if map.len() > MAX_ENTRIES_PER_CARD {
        return Err("jmap_response_too_large");
    }
    for entry in map.values() {
        let full = clean(entry["full"].as_str().unwrap_or(""), MAX_TEXT);
        let mut parts = Vec::new();
        if full.is_empty()
            && let Some(components) = entry["components"].as_array()
        {
            if components.len() > MAX_COMPONENTS {
                return Err("jmap_response_too_large");
            }
            for component in components {
                if let Some(value) = component["value"].as_str() {
                    let value = clean(value, MAX_TEXT);
                    if !value.is_empty() {
                        parts.push(value);
                    }
                }
            }
        }
        let value = if full.is_empty() {
            parts.join(", ")
        } else {
            full
        };
        if value.is_empty() {
            continue;
        }
        rows.push(json!({"value": value, "label": label_of(entry)}));
    }
    Ok(json!(rows))
}

fn list_row(card: &Value, source: &str) -> Result<Value, &'static str> {
    let id = bounded_id(card["id"].as_str().ok_or("jmap_invalid_response")?)?;
    let mut emails = Vec::new();
    let mut seen = HashSet::new();
    if let Some(map) = card["emails"].as_object() {
        if map.len() > MAX_EMAILS_PER_CARD {
            return Err("jmap_response_too_large");
        }
        for entry in map.values() {
            if let Some(address) = entry["address"].as_str() {
                let address = clean(address, MAX_TEXT);
                if !address.is_empty() && seen.insert(address.to_lowercase()) {
                    emails.push(Value::String(address));
                }
            }
        }
    }
    Ok(json!({
        "id": id,
        "name": display_name(&card["name"]),
        "emails": emails,
        "source": source,
    }))
}

fn contact_of(card: &Value) -> Result<Value, &'static str> {
    let id = bounded_id(card["id"].as_str().ok_or("jmap_invalid_response")?)?;
    let source = match card["addressBookIds"]
        .as_object()
        .and_then(|map| map.keys().next())
    {
        Some(key) => bounded_id(key)?.to_owned(),
        None => String::new(),
    };
    Ok(json!({
        "id": id,
        "source": source,
        "name": display_name(&card["name"]),
        "emails": labelled(&card["emails"], "address")?,
        "phones": labelled(&card["phones"], "number")?,
        "addresses": postal(&card["addresses"])?,
    }))
}

fn sort_key(row: &Value) -> (String, String, String) {
    let name = row["name"].as_str().unwrap_or("").to_lowercase();
    let email = row["emails"]
        .as_array()
        .and_then(|list| list.first())
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_lowercase();
    let id = row["id"].as_str().unwrap_or("").to_owned();
    (name, email, id)
}

fn normalize(cards: &[Value]) -> Result<Value, &'static str> {
    let mut rows = Vec::new();
    for card in cards {
        // One hostile card must not cost the whole address book: malformed
        // names are cleaned, malformed addresses are left for the shared
        // contact validation to drop.
        let name = clean(card["name"]["full"].as_str().unwrap_or(""), MAX_TEXT);
        let Some(emails) = card["emails"].as_object() else {
            continue;
        };
        if emails.len() > MAX_EMAILS_PER_CARD {
            return Err("jmap_response_too_large");
        }
        for email in emails.values() {
            let Some(address) = email["address"].as_str() else {
                continue;
            };
            if address.len() > MAX_TEXT {
                continue;
            }
            rows.push(json!({"name": &name, "email": address}));
            if rows.len() > MAX_REMOTE_CONTACTS {
                return Err("jmap_response_too_large");
            }
        }
    }
    crate::contacts::merge(&json!([]), &Value::Array(rows))
}

fn bounded_id(value: &str) -> Result<&str, &'static str> {
    if value.is_empty()
        || value.len() > 8192
        || value.chars().any(|character| {
            character.is_control()
                || matches!(
                    character,
                    '\u{061c}'
                        | '\u{200e}'
                        | '\u{200f}'
                        | '\u{202a}'..='\u{202e}'
                        | '\u{2066}'..='\u{2069}'
                )
        })
    {
        return Err("jmap_invalid_response");
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(document: Value) -> mailbox::Snapshot {
        mailbox::Snapshot {
            document,
            boxes: vec![],
            credential: json!({}),
            address: "user@example.test".into(),
            account: "mail-account".into(),
            roles: json!({}),
            slots: std::sync::Arc::new(tokio::sync::Semaphore::new(1)),
            uploads: std::sync::Arc::new(tokio::sync::Semaphore::new(1)),
        }
    }

    #[test]
    fn contacts_primary_account_is_independent_from_mail() {
        let value = snapshot(json!({
            "capabilities": {CONTACTS: {}},
            "primaryAccounts": {CONTACTS: "contacts-account"},
            "accounts": {"contacts-account": {"accountCapabilities": {CONTACTS: {}}}}
        }));
        assert_eq!(contacts_account(&value), Ok("contacts-account"));
        let missing = snapshot(json!({"capabilities": {}, "primaryAccounts": {}, "accounts": {}}));
        assert_eq!(contacts_account(&missing), Err("contacts_not_supported"));
    }

    #[test]
    fn source_selection_prefers_readable_default_then_subscribed() {
        let books = json!([
            {"id":"hidden","isDefault":true,"myRights":{"mayRead":false}},
            {"id":"subscribed","isSubscribed":true,"myRights":{"mayRead":true}},
            {"id":"default","isDefault":true,"myRights":{"mayRead":true}}
        ]);
        assert_eq!(
            choose_source(books.as_array().unwrap()),
            Ok("default".into())
        );
        assert_eq!(
            readable_book(books.as_array().unwrap(), "hidden"),
            Ok(false)
        );
        assert_eq!(
            readable_book(books.as_array().unwrap(), "default"),
            Ok(true)
        );
        assert_eq!(
            readable_book(books.as_array().unwrap(), "absent"),
            Ok(false)
        );
    }

    #[test]
    fn sources_report_rights_without_leaking_server_details() {
        let books = json!([
            {"id":"b","name":"Address Book","isDefault":true,"isSubscribed":true,"myRights":{"mayRead":true,"mayWrite":true}},
            {"id":"c","name":"Trusted\nSenders","isSubscribed":false,"myRights":{"mayRead":true}}
        ]);
        assert_eq!(
            sources_of(books.as_array().unwrap()).unwrap(),
            json!({"sources": [
                {"id":"b","name":"Address Book","default":true,"subscribed":true,"readOnly":false},
                {"id":"c","name":"TrustedSenders","default":false,"subscribed":false,"readOnly":true}
            ]})
        );
    }

    #[test]
    fn cards_expand_emails_and_reuse_contact_validation() {
        let cards = json!([{
            "id":"one",
            "name":{"full":"Alice"},
            "emails":{
                "a":{"address":"Alice@example.com"},
                "b":{"address":"alice@example.com"},
                "bad":{"address":"not-an-address"}
            }
        }]);
        let rows = normalize(cards.as_array().unwrap()).unwrap();
        assert_eq!(rows, json!([{"name":"Alice","email":"Alice@example.com"}]));
    }

    #[test]
    fn sanitizes_hostile_names_and_rejects_oversized_email_maps() {
        let cards = json!([{"id":"one","name":{"full":"Alice\nInjected"},"emails":{"a":{"address":"alice@example.com"},"b":{"address":"not-an-address"}}}]);
        assert_eq!(
            normalize(cards.as_array().unwrap()).unwrap(),
            json!([{"name":"AliceInjected","email":"alice@example.com"}])
        );
        let mut emails = serde_json::Map::new();
        for n in 0..=MAX_EMAILS_PER_CARD {
            emails.insert(
                n.to_string(),
                json!({"address":format!("a{n}@example.com")}),
            );
        }
        let cards = vec![json!({"id":"one","name":{"full":"Alice"},"emails":emails})];
        assert_eq!(normalize(&cards), Err("jmap_response_too_large"));
    }

    #[test]
    fn list_rows_carry_identity_display_name_and_addresses() {
        let card = json!({
            "id":"one",
            "name":{"components":[{"kind":"given","value":"Jane"},{"kind":"surname","value":"Doe"}]},
            "emails":{"a":{"address":"jane@example.test"},"b":{"address":"j.doe@example.test"}}
        });
        assert_eq!(
            list_row(&card, "book").unwrap(),
            json!({"id":"one","name":"Jane Doe","emails":["jane@example.test","j.doe@example.test"],"source":"book"})
        );
        let ordered = vec![
            list_row(&json!({"id":"b","name":{"full":"Zoe"},"emails":{"a":{"address":"z@example.test"}}}), "book").unwrap(),
            list_row(&json!({"id":"a","name":{"full":"adam"},"emails":{"a":{"address":"a@example.test"}}}), "book").unwrap(),
        ];
        let mut sorted = ordered.clone();
        sorted.sort_by(|a, b| sort_key(a).cmp(&sort_key(b)));
        assert_eq!(sorted[0]["id"], "a", "ordering is name-insensitive");
        assert_eq!(sorted[1]["id"], "b");
    }

    #[test]
    fn detail_projects_names_labels_and_postal_lines() {
        let card = json!({
            "id":"one",
            "addressBookIds":{"book":true},
            "name":{"full":"Jane Doe","components":[
                {"kind":"given","value":"Jane"},{"kind":"surname","value":"Doe"}
            ]},
            "emails":{"a":{"address":"jane@example.test","contexts":{"work":true}}},
            "phones":{"a":{"number":"+49-1","label":"mobile"}},
            "addresses":{"a":{"components":[
                {"kind":"street","value":"Example Street 1"},
                {"kind":"locality","value":"Example City"}
            ]}}
        });
        assert_eq!(
            contact_of(&card).unwrap(),
            json!({
                "id":"one","source":"book","name":"Jane Doe",
                "emails":[{"value":"jane@example.test","label":"work"}],
                "phones":[{"value":"+49-1","label":"mobile"}],
                "addresses":[{"value":"Example Street 1, Example City","label":""}]
            })
        );
        let hostile = json!({"id":"one","addressBookIds":{"book":true},
            "name":{"full":"A\u{0}B"},"emails":{"a":{"address":"a@example.test"}},
            "phones":{},"addresses":{}});
        assert_eq!(contact_of(&hostile).unwrap()["name"], "AB");
    }

    struct Peer(std::process::Child);
    impl Drop for Peer {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    async fn fixture_session() -> (Peer, Session, String) {
        use std::io::{BufRead, BufReader};
        let mut peer = Peer(
            std::process::Command::new("python3")
                .arg(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/src/providers/jmap/mailbox_tls_test.py"
                ))
                .stdout(std::process::Stdio::piped())
                .spawn()
                .unwrap(),
        );
        let mut output = BufReader::new(peer.0.stdout.take().unwrap());
        let mut port = String::new();
        output.read_line(&mut port).unwrap();
        let port: u16 = port.trim().parse().unwrap();
        let mut cert = String::new();
        output.read_line(&mut cert).unwrap();
        let session = Session::with_test_certificate(&std::fs::read(cert.trim()).unwrap()).unwrap();
        let document = json!({
            "apiUrl": format!("https://localhost:{port}/api"),
            "downloadUrl": format!("https://localhost:{port}/blob/{{blobId}}"),
            "uploadUrl": format!("https://localhost:{port}/upload"),
            "eventSourceUrl": format!("https://localhost:{port}/events"),
            "state": "s1",
            "capabilities": {
                mailbox::CORE: {"maxObjectsInGet": 1},
                mailbox::MAIL: {},
                CONTACTS: {}
            },
            "accounts": {
                "mail-account": {"accountCapabilities": {mailbox::MAIL: {"emailQuerySortOptions":["receivedAt"]}}},
                "contacts-account": {"accountCapabilities": {CONTACTS: {}}}
            },
            "primaryAccounts": {
                mailbox::MAIL: "mail-account",
                CONTACTS: "contacts-account"
            }
        });
        session
            .install_snapshot_for_test(
                "jmap:user@example.test",
                document,
                vec![],
                json!({"scheme":"basic","username":"user","secret":"synthetic"}),
                "user@example.test",
            )
            .await
            .unwrap();
        let account = "jmap:user@example.test".to_owned();
        (peer, session, account)
    }

    #[tokio::test]
    async fn reads_contact_suggestions_through_native_jmap_transport() {
        let (_peer, session, account) = fixture_session().await;
        let rows = session.contact_suggestions(&account).await.unwrap();
        assert_eq!(
            rows,
            json!([
                {"name":"Alice","email":"alice@example.test"},
                {"name":"Bob","email":"bob@example.test"}
            ])
        );
        assert!(!rows.to_string().contains("synthetic-secret"));
    }

    #[tokio::test]
    async fn reads_address_books_list_and_detail_through_native_transport() {
        let (_peer, session, account) = fixture_session().await;
        let sources = session.contact_sources(&account).await.unwrap();
        assert_eq!(
            sources,
            json!({"sources":[
                {"id":"hidden","name":"Hidden","default":true,"subscribed":true,"readOnly":true},
                {"id":"book","name":"Contacts","default":true,"subscribed":true,"readOnly":false}
            ]})
        );
        let page = session
            .contact_list(&account, "book", "", 50, 0)
            .await
            .unwrap();
        assert_eq!(page["total"], 2);
        assert_eq!(page["contacts"][0]["name"], "Alice");
        assert_eq!(page["contacts"][0]["emails"][0], "alice@example.test");
        assert_eq!(page["contacts"][1]["name"], "Bob");
        assert_eq!(page["contacts"][1]["emails"].as_array().unwrap().len(), 1);
        assert!(!page.to_string().contains("synthetic-secret"));

        let searched = session
            .contact_list(&account, "book", "zzz-no-match", 50, 0)
            .await
            .unwrap();
        assert_eq!(searched["total"], 0);
        assert!(
            session
                .contact_list(&account, "trusted-senders", "", 50, 0)
                .await
                .is_err(),
            "an unknown or unreadable source must be refused"
        );

        let contact = session.contact_detail(&account, "contact-2").await.unwrap();
        assert_eq!(contact["contact"]["name"], "Bob");
        assert_eq!(contact["contact"]["source"], "book");
        assert_eq!(contact["contact"]["emails"][0]["value"], "bob@example.test");
        assert!(
            session
                .contact_detail(&account, "missing")
                .await
                .is_err_and(|error| error == "contacts_not_found")
        );
    }
}
