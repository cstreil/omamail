//! Protocol-neutral contact RPCs: local harvesting plus JMAP address books.
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
        "contacts.get" => detail(session, fields).await,
        _ => Err("unknown_method"),
    }
}

async fn suggest(session: &Session, fields: &Map<String, Value>) -> Result<Value, &'static str> {
    allow(fields, &["accountId"])?;
    if !fields.contains_key("accountId") {
        return tokio::task::spawn_blocking(crate::contacts::suggest)
            .await
            .map_err(|_| "worker_failed")?;
    }
    let account = account_id(fields)?;
    let (provider, local) = account_and_local(account).await?;
    if provider != "jmap" {
        return Ok(local);
    }
    let remote = session.jmap.contact_suggestions(account).await?;
    crate::contacts::merge(&local, &remote)
}

async fn sources(session: &Session, fields: &Map<String, Value>) -> Result<Value, &'static str> {
    allow(fields, &["accountId"])?;
    let account = account_id(fields)?;
    let (provider, _) = account_and_local(account).await?;
    if provider != "jmap" {
        return Ok(json!({"sources": []}));
    }
    session.jmap.contact_sources(account).await
}

async fn list(session: &Session, fields: &Map<String, Value>) -> Result<Value, &'static str> {
    allow(
        fields,
        &["accountId", "source", "query", "limit", "position"],
    )?;
    let account = account_id(fields)?;
    let source = text(fields, "source", MAX_SOURCE)?
        .filter(|value| !value.is_empty())
        .ok_or("invalid_params")?;
    let query = text(fields, "query", MAX_QUERY)?.unwrap_or("");
    let limit = number(fields, "limit", DEFAULT_LIMIT, MAX_LIMIT)?;
    let position = number(fields, "position", 0, usize::MAX)?;
    let (provider, _) = account_and_local(account).await?;
    if provider != "jmap" {
        return Ok(json!({"contacts": [], "total": 0, "position": 0}));
    }
    session
        .jmap
        .contact_list(account, source, query, limit, position)
        .await
}

async fn detail(session: &Session, fields: &Map<String, Value>) -> Result<Value, &'static str> {
    allow(fields, &["accountId", "id"])?;
    let account = account_id(fields)?;
    let id = text(fields, "id", MAX_SOURCE)?
        .filter(|value| !value.is_empty())
        .ok_or("invalid_params")?;
    let (provider, _) = account_and_local(account).await?;
    if provider != "jmap" {
        return Err("contacts_not_supported");
    }
    session.jmap.contact_detail(account, id).await
}

// The account registry and the local harvest are both blocking file work, so
// they share one worker and never stall an async worker.
async fn account_and_local(account: &str) -> Result<(String, Value), &'static str> {
    let lookup = account.to_owned();
    tokio::task::spawn_blocking(move || {
        let accounts = crate::account::list_readonly()?;
        let entry = accounts["accounts"]
            .as_array()
            .and_then(|entries| entries.iter().find(|entry| entry["id"] == lookup))
            .ok_or("contacts_account_unknown")?;
        let provider = entry["provider"].as_str().ok_or("accounts_invalid")?;
        Ok::<_, &'static str>((provider.to_owned(), crate::contacts::suggest()?))
    })
    .await
    .map_err(|_| "worker_failed")?
}

fn allow(fields: &Map<String, Value>, allowed: &[&str]) -> Result<(), &'static str> {
    if fields.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err("invalid_params");
    }
    Ok(())
}

fn account_id(fields: &Map<String, Value>) -> Result<&str, &'static str> {
    text(fields, "accountId", MAX_ACCOUNT_ID)?
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
            if value.len() <= max && !value.chars().any(char::is_control) =>
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
    fn contact_parameters_are_strict_and_bounded() {
        for forbidden in [
            json!({"accountId": ""}),
            json!({"accountId": "a@example.test\nInjected"}),
            json!({"accountId": 7}),
            json!({}),
        ] {
            let fields = params(forbidden);
            assert_eq!(account_id(&fields), Err("invalid_params"));
        }
        assert_eq!(
            account_id(&params(json!({"accountId": "a@example.test"}))),
            Ok("a@example.test")
        );
        let fields = params(json!({"accountId": "a", "source": "book", "query": "\u{0}"}));
        assert_eq!(text(&fields, "source", MAX_SOURCE), Ok(Some("book")));
        assert_eq!(text(&fields, "query", MAX_QUERY), Err("invalid_params"));
        let fields = params(json!({"limit": 201}));
        assert_eq!(
            number(&fields, "limit", DEFAULT_LIMIT, MAX_LIMIT),
            Err("invalid_params")
        );
        let fields = params(json!({"limit": 200}));
        assert_eq!(number(&fields, "limit", DEFAULT_LIMIT, MAX_LIMIT), Ok(200));
        let fields = params(json!({"position": 3}));
        assert_eq!(number(&fields, "position", 0, usize::MAX), Ok(3));
        assert_eq!(
            number(&params(json!({})), "limit", DEFAULT_LIMIT, MAX_LIMIT),
            Ok(DEFAULT_LIMIT)
        );
        assert_eq!(
            allow(
                &params(json!({"accountId": "a", "extra": true})),
                &["accountId"]
            ),
            Err("invalid_params")
        );
        assert_eq!(
            allow(
                &params(json!({"accountId": "a", "source": "book"})),
                &["accountId", "source"]
            ),
            Ok(())
        );
        assert_eq!(
            allow(&params(json!({"accountId": "a"})), &["accountId"]),
            Ok(())
        );
    }
}
