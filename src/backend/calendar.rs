//! Protocol-neutral, read-only calendar RPCs.
use super::Session;
use serde_json::{Map, Value, json};
use std::collections::HashSet;

const MAX_ACCOUNT_ID: usize = 512;
const MAX_SOURCE_ID: usize = 32768;
const MAX_SOURCES: usize = 256;
const MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;
const MAX_RANGE_MS: i64 = 366 * 24 * 60 * 60 * 1000;

pub(super) async fn call(
    session: &Session,
    method: &str,
    params: &Value,
) -> Result<Value, &'static str> {
    let fields = params.as_object().ok_or("invalid_params")?;
    match method {
        "calendar.sources" => sources(session, fields).await,
        "calendar.events" => events(session, fields).await,
        _ => Err("unknown_method"),
    }
}

async fn sources(session: &Session, fields: &Map<String, Value>) -> Result<Value, &'static str> {
    allow(fields, &["accountId"])?;
    let account = account_id(fields)?;
    if account_provider(account).await? != "jmap" {
        return Ok(json!({"sources": []}));
    }
    match session.jmap.calendar_sources(account).await {
        Err("calendar_not_supported") => Ok(json!({"sources": []})),
        result => result,
    }
}

async fn events(session: &Session, fields: &Map<String, Value>) -> Result<Value, &'static str> {
    allow(fields, &["accountId", "sources", "start", "end"])?;
    let account = account_id(fields)?;
    let sources = source_ids(fields)?;
    let start = epoch_ms(fields, "start")?;
    let end = epoch_ms(fields, "end")?;
    if end <= start || end.checked_sub(start).is_none_or(|span| span > MAX_RANGE_MS) {
        return Err("invalid_params");
    }
    if account_provider(account).await? != "jmap" {
        return Ok(json!({"events": []}));
    }
    match session
        .jmap
        .calendar_events(account, &sources, start, end)
        .await
    {
        Err("calendar_not_supported") => Ok(json!({"events": []})),
        result => result,
    }
}

async fn account_provider(account: &str) -> Result<String, &'static str> {
    let lookup = account.to_owned();
    tokio::task::spawn_blocking(move || {
        let accounts = crate::account::list_readonly()?;
        Ok::<_, &'static str>(provider_from_accounts(&accounts, &lookup)?.to_owned())
    })
    .await
    .map_err(|_| "worker_failed")?
}

fn provider_from_accounts<'a>(accounts: &'a Value, account: &str) -> Result<&'a str, &'static str> {
    accounts["accounts"]
        .as_array()
        .and_then(|entries| entries.iter().find(|entry| entry["id"] == account))
        .ok_or("calendar_account_unknown")?["provider"]
        .as_str()
        .ok_or("accounts_invalid")
}

fn allow(fields: &Map<String, Value>, allowed: &[&str]) -> Result<(), &'static str> {
    if fields.len() != allowed.len()
        || fields.keys().any(|key| !allowed.contains(&key.as_str()))
    {
        return Err("invalid_params");
    }
    Ok(())
}

fn account_id(fields: &Map<String, Value>) -> Result<&str, &'static str> {
    bounded_text(fields.get("accountId"), MAX_ACCOUNT_ID)
}

fn source_ids(fields: &Map<String, Value>) -> Result<Vec<String>, &'static str> {
    let values = fields
        .get("sources")
        .and_then(Value::as_array)
        .filter(|values| !values.is_empty() && values.len() <= MAX_SOURCES)
        .ok_or("invalid_params")?;
    let mut seen = HashSet::with_capacity(values.len());
    let mut sources = Vec::with_capacity(values.len());
    for value in values {
        let source = bounded_text(Some(value), MAX_SOURCE_ID)?;
        if !seen.insert(source) {
            return Err("invalid_params");
        }
        sources.push(source.to_owned());
    }
    Ok(sources)
}

fn bounded_text(value: Option<&Value>, max: usize) -> Result<&str, &'static str> {
    value
        .and_then(Value::as_str)
        .filter(|value| {
            !value.is_empty()
                && value.len() <= max
                && !value.chars().any(|character| {
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
        })
        .ok_or("invalid_params")
}

fn epoch_ms(fields: &Map<String, Value>, key: &str) -> Result<i64, &'static str> {
    fields
        .get(key)
        .and_then(Value::as_i64)
        .filter(|value| *value >= -MAX_SAFE_INTEGER && *value <= MAX_SAFE_INTEGER)
        .ok_or("invalid_params")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields(value: Value) -> Map<String, Value> {
        value.as_object().unwrap().clone()
    }

    #[test]
    fn account_lookup_has_calendar_specific_stable_error() {
        let accounts = json!({"accounts":[
            {"id":"jmap:one@example.test","provider":"jmap"},
            {"id":"imap:two@example.test","provider":"imap"}
        ]});
        assert_eq!(provider_from_accounts(&accounts, "jmap:one@example.test"), Ok("jmap"));
        assert_eq!(provider_from_accounts(&accounts, "missing"), Err("calendar_account_unknown"));
    }

    #[test]
    fn calendar_params_are_exact_unique_and_bounded() {
        assert_eq!(allow(&fields(json!({"accountId":"a"})), &["accountId"]), Ok(()));
        assert_eq!(allow(&fields(json!({"accountId":"a","extra":true})), &["accountId"]), Err("invalid_params"));
        assert_eq!(account_id(&fields(json!({"accountId":"a\n"}))), Err("invalid_params"));
        assert_eq!(source_ids(&fields(json!({"sources":[]}))), Err("invalid_params"));
        assert_eq!(source_ids(&fields(json!({"sources":["one","one"]}))), Err("invalid_params"));
        assert_eq!(source_ids(&fields(json!({"sources":["one","two"]}))), Ok(vec!["one".into(), "two".into()]));
        assert_eq!(epoch_ms(&fields(json!({"start":MAX_SAFE_INTEGER})), "start"), Ok(MAX_SAFE_INTEGER));
        assert_eq!(epoch_ms(&fields(json!({"start":9_007_199_254_740_992_i64})), "start"), Err("invalid_params"));
        assert_eq!(epoch_ms(&fields(json!({"start":1.5})), "start"), Err("invalid_params"));
    }

    #[tokio::test]
    async fn event_range_validation_happens_before_account_lookup() {
        let session = Session::default();
        for params in [
            json!({"accountId":"missing","sources":["x"],"start":2,"end":2}),
            json!({"accountId":"missing","sources":["x"],"start":0,"end":MAX_RANGE_MS+1}),
            json!({"accountId":"missing","sources":[],"start":0,"end":1}),
            json!({"accountId":"missing","sources":["x"],"start":0,"end":1,"extra":true}),
        ] {
            assert_eq!(call(&session, "calendar.events", &params).await, Err("invalid_params"));
        }
    }
}
