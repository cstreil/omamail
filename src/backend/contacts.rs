//! Protocol-neutral read-only contact RPCs. The API-5 empty suggestion call stays local-only.
use super::Session;
use serde_json::{Map, Value, json};

const MAX_ACCOUNT_ID: usize = 512;
const MAX_SOURCE: usize = 8192;
const MAX_QUERY: usize = 256;
const MAX_LIMIT: usize = 200;
const DEFAULT_LIMIT: usize = 50;

pub(super) async fn call(
    session: &Session,
    method: &str,
    params: &Value,
) -> Result<Value, &'static str> {
    let fields = params.as_object().ok_or("invalid_params")?;
    match method {
        "contacts.suggest" => suggest(session, fields).await,
        "contacts.sources" => sources(session, fields).await,
        "contacts.list" => list(session, fields).await,
        "contacts.detail" => detail(session, fields).await,
        _ => Err("unknown_method"),
    }
}

async fn suggest(session: &Session, fields: &Map<String, Value>) -> Result<Value, &'static str> {
    allow(fields, &["accountId"])?;
    if !fields.contains_key("accountId") {
        // Preserve the released API-5 behavior, including its local-only errors.
        return tokio::task::spawn_blocking(crate::contacts::suggest)
            .await
            .map_err(|_| "worker_failed")?;
    }
    let account = account_id(fields)?;
    let (provider, local) = account_and_local(account).await?;
    if provider != "jmap" {
        return Ok(local);
    }
    match session.jmap.contact_suggestions(account).await {
        Ok(remote) => crate::contacts::merge(&local, &remote),
        Err("contacts_not_supported") => Ok(local),
        Err(error) => Err(error),
    }
}

async fn sources(session: &Session, fields: &Map<String, Value>) -> Result<Value, &'static str> {
    allow(fields, &["accountId"])?;
    let account = account_id(fields)?;
    if account_provider(account).await? != "jmap" {
        return Ok(json!({"sources": []}));
    }
    match session.jmap.contact_sources(account).await {
        Err("contacts_not_supported") => Ok(json!({"sources": []})),
        result => result,
    }
}

async fn list(session: &Session, fields: &Map<String, Value>) -> Result<Value, &'static str> {
    allow(
        fields,
        &["accountId", "source", "query", "limit", "position"],
    )?;
    let account = account_id(fields)?;
    let source = required_text(fields, "source", MAX_SOURCE)?;
    let query = text(fields, "query", MAX_QUERY)?.unwrap_or("");
    // A zero limit returns an empty page while preserving the actual total.
    let limit = number(fields, "limit", DEFAULT_LIMIT, MAX_LIMIT)?;
    let position = number(fields, "position", 0, usize::MAX)?;
    if account_provider(account).await? != "jmap" {
        return Ok(json!({"contacts": [], "total": 0, "position": 0}));
    }
    match session
        .jmap
        .contact_list(account, source, query, limit, position)
        .await
    {
        Err("contacts_not_supported") => Ok(json!({"contacts": [], "total": 0, "position": 0})),
        result => result,
    }
}

async fn detail(session: &Session, fields: &Map<String, Value>) -> Result<Value, &'static str> {
    allow(fields, &["accountId", "id"])?;
    let account = account_id(fields)?;
    let id = required_text(fields, "id", MAX_SOURCE)?;
    if account_provider(account).await? != "jmap" {
        return Err("contacts_not_supported");
    }
    session.jmap.contact_detail(account, id).await
}

// Registry and local contact collection are blocking filesystem work. Remote-only
// directory reads must not harvest local address books to identify an account.
async fn account_provider(account: &str) -> Result<String, &'static str> {
    let lookup = account.to_owned();
    tokio::task::spawn_blocking(move || {
        let accounts = crate::account::list_readonly()?;
        Ok::<_, &'static str>(provider_from_accounts(&accounts, &lookup)?.to_owned())
    })
    .await
    .map_err(|_| "worker_failed")?
}

async fn account_and_local(account: &str) -> Result<(String, Value), &'static str> {
    let lookup = account.to_owned();
    tokio::task::spawn_blocking(move || {
        let accounts = crate::account::list_readonly()?;
        let provider = provider_from_accounts(&accounts, &lookup)?.to_owned();
        Ok::<_, &'static str>((provider, crate::contacts::suggest()?))
    })
    .await
    .map_err(|_| "worker_failed")?
}

fn provider_from_accounts<'a>(accounts: &'a Value, account: &str) -> Result<&'a str, &'static str> {
    accounts["accounts"]
        .as_array()
        .and_then(|entries| entries.iter().find(|entry| entry["id"] == account))
        .ok_or("contacts_account_unknown")?["provider"]
        .as_str()
        .ok_or("accounts_invalid")
}

fn allow(fields: &Map<String, Value>, allowed: &[&str]) -> Result<(), &'static str> {
    if fields.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err("invalid_params");
    }
    Ok(())
}

fn account_id(fields: &Map<String, Value>) -> Result<&str, &'static str> {
    required_text(fields, "accountId", MAX_ACCOUNT_ID)
}

fn required_text<'a>(
    fields: &'a Map<String, Value>,
    key: &str,
    max: usize,
) -> Result<&'a str, &'static str> {
    text(fields, key, max)?
        .filter(|value| !value.is_empty())
        .ok_or("invalid_params")
}

fn text<'a>(
    fields: &'a Map<String, Value>,
    key: &str,
    max: usize,
) -> Result<Option<&'a str>, &'static str> {
    match fields.get(key) {
        None => Ok(None),
        Some(Value::String(value))
            if value.len() <= max
                && !value.chars().any(|c| {
                    c.is_control()
                        || matches!(
                            c,
                            '\u{061c}'
                                | '\u{200e}'
                                | '\u{200f}'
                                | '\u{202a}'..='\u{202e}'
                                | '\u{2066}'..='\u{2069}'
                        )
                }) =>
        {
            Ok(Some(value))
        }
        Some(_) => Err("invalid_params"),
    }
}

fn number(
    fields: &Map<String, Value>,
    key: &str,
    fallback: usize,
    max: usize,
) -> Result<usize, &'static str> {
    match fields.get(key) {
        None => Ok(fallback),
        Some(value) => value
            .as_u64()
            .filter(|value| *value <= max as u64)
            .map(|value| value as usize)
            .ok_or("invalid_params"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(value: Value) -> Map<String, Value> {
        value.as_object().unwrap().clone()
    }

    #[test]
    fn contact_parameters_refuse_controls_oversize_and_unknown_fields_before_lookup() {
        for forbidden in [
            json!({"accountId": ""}),
            json!({"accountId": "a@example.test\nInjected"}),
            json!({"accountId": "\u{202e}bad"}),
            json!({"accountId": 7}),
            json!({}),
        ] {
            assert_eq!(account_id(&params(forbidden)), Err("invalid_params"));
        }
        assert_eq!(
            account_id(&params(json!({"accountId": "jmap:one@example.test"}))),
            Ok("jmap:one@example.test")
        );
        let fields = params(json!({"accountId": "a", "source": "book", "query": "\u{0}"}));
        assert_eq!(text(&fields, "source", MAX_SOURCE), Ok(Some("book")));
        assert_eq!(text(&fields, "query", MAX_QUERY), Err("invalid_params"));
        assert_eq!(
            number(
                &params(json!({"limit": 201})),
                "limit",
                DEFAULT_LIMIT,
                MAX_LIMIT
            ),
            Err("invalid_params")
        );
        assert_eq!(
            number(
                &params(json!({"limit": 0})),
                "limit",
                DEFAULT_LIMIT,
                MAX_LIMIT
            ),
            Ok(0)
        );
        assert_eq!(
            number(
                &params(json!({"limit": 200})),
                "limit",
                DEFAULT_LIMIT,
                MAX_LIMIT
            ),
            Ok(200)
        );
        assert_eq!(
            allow(
                &params(json!({"accountId": "a", "extra": true})),
                &["accountId"]
            ),
            Err("invalid_params")
        );
    }

    #[test]
    fn provider_selection_requires_an_exact_registry_entry() {
        let accounts = json!({"accounts":[
            {"id":"jmap:one@example.test","provider":"jmap"},
            {"id":"imap:two@example.test","provider":"imap"}
        ]});
        assert_eq!(
            provider_from_accounts(&accounts, "jmap:one@example.test"),
            Ok("jmap")
        );
        assert_eq!(
            provider_from_accounts(&accounts, "jmap:other@example.test"),
            Err("contacts_account_unknown")
        );
        assert_eq!(
            provider_from_accounts(&json!({"accounts":[{"id":"broken"}]}), "broken"),
            Err("accounts_invalid")
        );
    }

    #[tokio::test]
    async fn dispatcher_reads_synthetic_jmap_contacts_without_storage_or_remote_writes() {
        use crate::mail::tests::{account_fixture, fixture_tree, isolated};
        use std::{
            fs,
            io::{BufRead, BufReader},
            process::{Child, Command, Stdio},
            sync::Arc,
        };

        if isolated() {
            return;
        }
        struct Peer(Child);
        impl Drop for Peer {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        const ACCOUNT: &str = "jmap:user@example.test";
        let fixture = account_fixture(json!({"version":1,"activeId":ACCOUNT,
            "accounts":[{"provider":"jmap","email":"user@example.test"}]}));
        let mut peer = Peer(
            Command::new("python3")
                .arg(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/src/providers/jmap/mailbox_tls_test.py"
                ))
                .stdout(Stdio::piped())
                .spawn()
                .unwrap(),
        );
        let mut output = BufReader::new(peer.0.stdout.take().unwrap());
        let mut port = String::new();
        output.read_line(&mut port).unwrap();
        let port: u16 = port.trim().parse().unwrap();
        let mut cert_path = String::new();
        output.read_line(&mut cert_path).unwrap();
        let cert = fs::read(cert_path.trim()).unwrap();
        let jmap = Arc::new(crate::providers::jmap::Session::with_test_certificate(&cert).unwrap());
        let document = json!({
            "apiUrl":format!("https://localhost:{port}/api"),
            "downloadUrl":format!("https://localhost:{port}/blob/{{blobId}}"),
            "uploadUrl":format!("https://localhost:{port}/upload"),
            "eventSourceUrl":format!("https://localhost:{port}/events"),
            "state":"s1",
            "capabilities":{
                "urn:ietf:params:jmap:core":{"maxObjectsInGet":1},
                "urn:ietf:params:jmap:mail":{},
                "urn:ietf:params:jmap:contacts":{}
            },
            "accounts":{
                "mail-account":{"accountCapabilities":{"urn:ietf:params:jmap:mail":{"emailQuerySortOptions":["receivedAt"]}}},
                "contacts-account":{"accountCapabilities":{"urn:ietf:params:jmap:contacts":{}}}
            },
            "primaryAccounts":{
                "urn:ietf:params:jmap:mail":"mail-account",
                "urn:ietf:params:jmap:contacts":"contacts-account"
            }
        });
        jmap.install_snapshot_for_test(
            ACCOUNT,
            document,
            vec![],
            json!({"scheme":"basic","username":"user","secret":"synthetic"}),
            "user@example.test",
        )
        .await
        .unwrap();
        let session = Session {
            jmap,
            ..Default::default()
        };
        let before = fixture_tree(&fixture.root);
        let local = session
            .dispatch("contacts.suggest", &json!({}))
            .await
            .unwrap();
        assert_eq!(local, json!([]));
        let suggestions = session
            .dispatch("contacts.suggest", &json!({"accountId":ACCOUNT}))
            .await
            .unwrap();
        assert_eq!(suggestions.as_array().unwrap().len(), 2);
        assert_eq!(suggestions[0]["email"], "alice@example.test");
        let sources = session
            .dispatch("contacts.sources", &json!({"accountId":ACCOUNT}))
            .await
            .unwrap();
        assert_eq!(sources["sources"][0]["id"], "book");
        let page = session
            .dispatch(
                "contacts.list",
                &json!({"accountId":ACCOUNT,"source":"book"}),
            )
            .await
            .unwrap();
        assert_eq!(page["total"], 2);
        let zero = session
            .dispatch(
                "contacts.list",
                &json!({"accountId":ACCOUNT,"source":"book","limit":0,"position":1}),
            )
            .await
            .unwrap();
        assert_eq!(zero, json!({"contacts":[],"total":2,"position":1}));
        let detail = session
            .dispatch(
                "contacts.detail",
                &json!({"accountId":ACCOUNT,"id":"contact-2"}),
            )
            .await
            .unwrap();
        assert_eq!(detail["contact"]["name"], "Bob");
        assert_eq!(detail["contact"]["source"], "book");
        assert!(!detail.to_string().contains("synthetic"));
        assert_eq!(
            fixture_tree(&fixture.root),
            before,
            "reads never write local data or call a credential helper"
        );

        let report = reqwest::Client::builder()
            .no_proxy()
            .add_root_certificate(reqwest::Certificate::from_pem(&cert).unwrap())
            .build()
            .unwrap()
            .get(format!("https://localhost:{port}/report"))
            .send()
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap();
        let report: Value = serde_json::from_slice(&report).unwrap();
        let requests = report.as_array().unwrap();
        assert!(!requests.is_empty());
        for request in requests {
            assert_eq!(request["method"], "POST");
            assert_eq!(request["path"], "/api");
            assert_eq!(request["authorization"], true);
            for call in request["calls"].as_array().unwrap() {
                assert!(matches!(
                    call[0].as_str(),
                    Some("AddressBook/get" | "ContactCard/query" | "ContactCard/get")
                ));
            }
        }
    }
}
