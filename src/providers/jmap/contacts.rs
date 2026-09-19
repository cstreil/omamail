//! Read-only JMAP Contacts projection for recipient suggestions.
use super::{Session, mailbox};
use serde_json::{Value, json};
use std::collections::HashSet;

const CORE: &str = "urn:ietf:params:jmap:core";
const CONTACTS: &str = "urn:ietf:params:jmap:contacts";
const MAX_REMOTE_CONTACTS: usize = 1000;
const MAX_EMAILS_PER_CARD: usize = 64;
const MAX_TEXT: usize = 4096;

impl Session {
    pub(crate) async fn contact_suggestions(
        &self,
        account_id: &str,
    ) -> Result<Value, &'static str> {
        let context = self.context(account_id)?;
        let result = tokio::time::timeout(std::time::Duration::from_secs(25), async {
            let snapshot = self.snapshot(account_id, &context).await?;
            let contacts_account = contacts_account(&snapshot)?;
            let source = self
                .contact_source(&context, &snapshot, contacts_account)
                .await?;
            let cards = self
                .contact_cards(&context, &snapshot, contacts_account, &source)
                .await?;
            normalize(&cards)
        })
        .await
        .unwrap_or(Err("jmap_timeout"));
        if matches!(result, Err("jmap_unauthorized")) {
            context
                .rejected
                .store(true, std::sync::atomic::Ordering::Release);
        }
        result
    }

    async fn contact_source(
        &self,
        context: &mailbox::Context,
        snapshot: &mailbox::Snapshot,
        account: &str,
    ) -> Result<String, &'static str> {
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
        choose_source(books)
    }

    async fn contact_cards(
        &self,
        context: &mailbox::Context,
        snapshot: &mailbox::Snapshot,
        account: &str,
        source: &str,
    ) -> Result<Vec<Value>, &'static str> {
        let result = self
            .api_using(
                context,
                snapshot,
                json!([["ContactCard/query", {
                    "accountId": account,
                    "filter": {"inAddressBook": source},
                    "position": 0,
                    "limit": MAX_REMOTE_CONTACTS
                }, "contacts-query"]]),
                json!([CORE, CONTACTS]),
            )
            .await?;
        let query = mailbox::argument(&result, "contacts-query", "ContactCard/query")?;
        let raw_ids = query["ids"].as_array().ok_or("jmap_invalid_response")?;
        if raw_ids.len() > MAX_REMOTE_CONTACTS {
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
        let mut cards = Vec::with_capacity(ids.len());
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
                        "properties": ["id", "addressBookIds", "name", "emails"]
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
            if returned.len() != expected.len() {
                return Err("jmap_invalid_response");
            }
        }
        Ok(cards)
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

fn choose_source(books: &[Value]) -> Result<String, &'static str> {
    if books.len() > 256 {
        return Err("jmap_response_too_large");
    }
    let readable = |book: &&Value| book["myRights"]["mayRead"] == true;
    let selected = books
        .iter()
        .filter(readable)
        .find(|book| book["isDefault"] == true)
        .or_else(|| {
            books
                .iter()
                .filter(readable)
                .find(|book| book["isSubscribed"] == true)
        })
        .ok_or("contacts_no_address_book")?;
    Ok(bounded_id(selected["id"].as_str().ok_or("jmap_invalid_response")?)?.to_owned())
}

fn normalize(cards: &[Value]) -> Result<Value, &'static str> {
    let mut rows = Vec::new();
    for card in cards {
        // One hostile card must not cost the whole address book: malformed
        // names are cleaned, malformed addresses are left for the shared
        // contact validation to drop.
        let name: String = card["name"]["full"]
            .as_str()
            .unwrap_or("")
            .chars()
            .filter(|character| !character.is_control())
            .take(MAX_TEXT)
            .collect();
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
            rows.push(json!({"name": name, "email": address}));
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

    struct Peer(std::process::Child);
    impl Drop for Peer {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    #[tokio::test]
    async fn reads_contact_suggestions_through_native_jmap_transport() {
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
        let rows = session
            .contact_suggestions("jmap:user@example.test")
            .await
            .unwrap();
        assert_eq!(
            rows,
            json!([
                {"name":"Alice","email":"alice@example.test"},
                {"name":"Bob","email":"bob@example.test"}
            ])
        );
        assert!(!rows.to_string().contains("synthetic-secret"));
    }
}
