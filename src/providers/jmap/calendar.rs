//! Read-only JMAP Calendars projection for the protocol-neutral calendar RPCs.
use super::{Session, mailbox};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Local, NaiveDate, NaiveDateTime, TimeZone, Utc};
use serde_json::{Value, json};
use std::{collections::HashSet, sync::Arc, time::Duration};

const CORE: &str = "urn:ietf:params:jmap:core";
const CALENDARS: &str = "urn:ietf:params:jmap:calendars";
const CALL_TIME: Duration = Duration::from_secs(25);
const TOKEN_VERSION: u64 = 1;
const MAX_ID: usize = 8192;
const MAX_TOKEN: usize = 32768;
const MAX_TEXT_BYTES: usize = 16384;
const MAX_TEXT_CHARS: usize = 4096;
const MAX_CALENDARS: usize = 256;
const MAX_EVENTS: usize = 10000;
const MAX_QUERY_PAGES: usize = 256;
const MAX_LOCATIONS: usize = 256;
const MAX_PARTICIPANTS: usize = 1024;
const MAX_OBJECT_FIELDS: usize = 256;
const EVENT_PROPERTIES: &[&str] = &[
    "id", "uid", "title", "description", "location", "locations", "participants", "status",
    "calendarIds", "showWithoutTime", "timeZone", "utcStart", "utcEnd", "start",
    "duration", "recurrenceId",
];

impl Session {
    pub(crate) async fn calendar_sources(&self, account_id: &str) -> Result<Value, &'static str> {
        let (context, snapshot, account) = self.calendar_setup(account_id).await?;
        let result = tokio::time::timeout(CALL_TIME, async {
            let calendars = self.calendars(&context, &snapshot, &account).await?;
            sources_of(account_id, &calendars)
        })
        .await
        .unwrap_or(Err("jmap_timeout"));
        note(&context, &result);
        result
    }

    pub(crate) async fn calendar_events(
        &self,
        account_id: &str,
        source_ids: &[String],
        start: i64,
        end: i64,
    ) -> Result<Value, &'static str> {
        let requested = source_ids
            .iter()
            .map(|token| decode_token(token, account_id))
            .collect::<Result<Vec<_>, _>>()?;
        let (context, snapshot, account) = self.calendar_setup(account_id).await?;
        let result = tokio::time::timeout(CALL_TIME, async {
            let calendars = self.calendars(&context, &snapshot, &account).await?;
            authorize_sources(&calendars, &requested)?;
            let ids = self
                .query_event_ids(&context, &snapshot, &account, start, end)
                .await?;
            let events = self
                .events(&context, &snapshot, &account, &ids)
                .await?;
            normalize_events(account_id, source_ids, &requested, &events, start, end)
        })
        .await
        .unwrap_or(Err("jmap_timeout"));
        note(&context, &result);
        result
    }

    async fn calendar_setup(
        &self,
        account_id: &str,
    ) -> Result<(Arc<mailbox::Context>, mailbox::Snapshot, String), &'static str> {
        let context = self.context(account_id)?;
        let setup = tokio::time::timeout(CALL_TIME, async {
            let snapshot = self.snapshot(account_id, &context).await?;
            let account = calendar_account(&snapshot)?.to_owned();
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

    async fn calendars(
        &self,
        context: &mailbox::Context,
        snapshot: &mailbox::Snapshot,
        account: &str,
    ) -> Result<Vec<Value>, &'static str> {
        let result = self
            .api_using(
                context,
                snapshot,
                json!([["Calendar/get", {
                    "accountId": account,
                    "ids": null,
                    "properties": ["id", "name", "isDefault", "isSubscribed", "isVisible", "myRights"]
                }, "calendar-sources"]]),
                json!([CORE, CALENDARS]),
            )
            .await?;
        let values = mailbox::argument(&result, "calendar-sources", "Calendar/get")?["list"]
            .as_array()
            .ok_or("jmap_invalid_response")?;
        if values.len() > MAX_CALENDARS {
            return Err("jmap_response_too_large");
        }
        let mut used = 0;
        let mut seen = HashSet::new();
        for value in values {
            charge_json_bytes(&mut used, value)?;
            validate_object(value)?;
            let id = bounded_id(value["id"].as_str().ok_or("jmap_invalid_response")?)?;
            if !seen.insert(id) {
                return Err("jmap_invalid_response");
            }
        }
        Ok(values.clone())
    }

    async fn query_event_ids(
        &self,
        context: &mailbox::Context,
        snapshot: &mailbox::Snapshot,
        account: &str,
        start: i64,
        end: i64,
    ) -> Result<Vec<String>, &'static str> {
        let mut ids = Vec::new();
        let mut seen = HashSet::new();
        let mut expected_total = None;
        let mut expected_state: Option<String> = None;
        let mut position = 0usize;
        let mut pages = 0usize;
        let mut bytes = 0usize;
        loop {
            pages += 1;
            if pages > MAX_QUERY_PAGES {
                return Err("jmap_response_too_large");
            }
            let remaining = MAX_EVENTS.saturating_sub(ids.len());
            if remaining == 0 && expected_total.is_some_and(|total| position < total) {
                return Err("calendar_too_many");
            }
            let result = self
                .api_using(
                    context,
                    snapshot,
                    json!([["CalendarEvent/query", {
                        "accountId": account,
                        "timeZone": "Etc/UTC",
                        "expandRecurrences": true,
                        "filter": {"after": utc_local(start)?, "before": utc_local(end)?},
                        "position": position,
                        "limit": remaining,
                        "calculateTotal": true
                    }, "calendar-query"]]),
                    json!([CORE, CALENDARS]),
                )
                .await?;
            let query = mailbox::argument(&result, "calendar-query", "CalendarEvent/query")?;
            let returned_position = usize_value(&query["position"])?;
            if returned_position != position {
                return Err("jmap_invalid_response");
            }
            let total = usize_value(&query["total"])?;
            if total > MAX_EVENTS {
                return Err("calendar_too_many");
            }
            if expected_total.is_some_and(|expected| expected != total) {
                return Err("jmap_invalid_response");
            }
            expected_total = Some(total);
            let state = bounded_id(query["queryState"].as_str().ok_or("jmap_invalid_response")?)?;
            if expected_state.as_deref().is_some_and(|expected| expected != state) {
                return Err("jmap_invalid_response");
            }
            expected_state = Some(state.to_owned());
            let page = query["ids"].as_array().ok_or("jmap_invalid_response")?;
            if page.len() > remaining {
                return Err("jmap_response_too_large");
            }
            for value in page {
                let id = bounded_id(value.as_str().ok_or("jmap_invalid_response")?)?;
                if !seen.insert(id.to_owned()) {
                    return Err("jmap_invalid_response");
                }
                charge_json_bytes(&mut bytes, value)?;
                ids.push(id.to_owned());
            }
            let next = position.checked_add(page.len()).ok_or("jmap_invalid_response")?;
            if next > total || (next < total && next == position) {
                return Err("jmap_invalid_response");
            }
            position = next;
            if position == total {
                return Ok(ids);
            }
        }
    }

    async fn events(
        &self,
        context: &mailbox::Context,
        snapshot: &mailbox::Snapshot,
        account: &str,
        ids: &[String],
    ) -> Result<Vec<Value>, &'static str> {
        let mut events = Vec::with_capacity(ids.len());
        let mut used = 0usize;
        for (index, chunk) in ids.chunks(snapshot.limit("maxObjectsInGet", 256)).enumerate() {
            let tag = format!("calendar-get-{index}");
            let result = self
                .api_using(
                    context,
                    snapshot,
                    json!([["CalendarEvent/get", {
                        "accountId": account,
                        "ids": chunk,
                        "properties": EVENT_PROPERTIES
                    }, tag]]),
                    json!([CORE, CALENDARS]),
                )
                .await?;
            let list = mailbox::argument(&result, &tag, "CalendarEvent/get")?["list"]
                .as_array()
                .ok_or("jmap_invalid_response")?;
            let expected: HashSet<_> = chunk.iter().map(String::as_str).collect();
            let mut returned = HashSet::new();
            for event in list {
                validate_object(event)?;
                let id = bounded_id(event["id"].as_str().ok_or("jmap_invalid_response")?)?;
                if !expected.contains(id) || !returned.insert(id.to_owned()) {
                    return Err("jmap_invalid_response");
                }
                charge_json_bytes(&mut used, event)?;
                events.push(event.clone());
            }
            if returned.len() != expected.len() {
                return Err("jmap_invalid_response");
            }
        }
        Ok(events)
    }
}

fn calendar_account(snapshot: &mailbox::Snapshot) -> Result<&str, &'static str> {
    if !snapshot.document["capabilities"][CALENDARS].is_object() {
        return Err("calendar_not_supported");
    }
    let account = snapshot.document["primaryAccounts"][CALENDARS]
        .as_str()
        .ok_or("calendar_not_supported")?;
    bounded_id(account)?;
    if !snapshot.document["accounts"][account]["accountCapabilities"][CALENDARS].is_object() {
        return Err("calendar_not_supported");
    }
    Ok(account)
}

fn visible_readable(calendar: &Value) -> bool {
    calendar["myRights"]["mayReadItems"] == true
        && (calendar["isSubscribed"] == true || calendar["isDefault"] == true)
}

fn sources_of(account_id: &str, calendars: &[Value]) -> Result<Value, &'static str> {
    if calendars.len() > MAX_CALENDARS {
        return Err("jmap_response_too_large");
    }
    let mut sources = Vec::new();
    for calendar in calendars {
        validate_object(calendar)?;
        let remote = bounded_id(calendar["id"].as_str().ok_or("jmap_invalid_response")?)?;
        if !visible_readable(calendar) {
            continue;
        }
        let name = clean_text(calendar["name"].as_str().unwrap_or(""))?;
        sources.push(json!({
            "id": encode_token(account_id, remote)?,
            "accountId": account_id,
            "kind": "account",
            "name": if name.is_empty() { "Calendar" } else { &name },
            "enabled": calendar["isSubscribed"] == true && calendar["isVisible"] == true,
            "default": calendar["isDefault"] == true,
            "subscribed": calendar["isSubscribed"] == true,
            "readOnly": true
        }));
    }
    Ok(json!({"sources": sources}))
}

fn authorize_sources(calendars: &[Value], requested: &[String]) -> Result<(), &'static str> {
    let mut allowed = HashSet::new();
    for calendar in calendars {
        let id = bounded_id(calendar["id"].as_str().ok_or("jmap_invalid_response")?)?;
        if visible_readable(calendar) {
            allowed.insert(id);
        }
    }
    if requested.iter().any(|id| !allowed.contains(id.as_str())) {
        return Err("calendar_source_unknown");
    }
    Ok(())
}

fn encode_token(account: &str, remote: &str) -> Result<String, &'static str> {
    bounded_id(remote)?;
    if account.is_empty() || account.len() > 512 || account.chars().any(char::is_control) {
        return Err("invalid_params");
    }
    let raw = serde_json::to_vec(&json!([TOKEN_VERSION, account, remote]))
        .map_err(|_| "jmap_invalid_response")?;
    let token = URL_SAFE_NO_PAD.encode(raw);
    if token.len() > MAX_TOKEN {
        return Err("jmap_response_too_large");
    }
    Ok(token)
}

fn decode_token(token: &str, account: &str) -> Result<String, &'static str> {
    if token.is_empty() || token.len() > MAX_TOKEN || token.chars().any(char::is_control) {
        return Err("calendar_source_unknown");
    }
    let raw = URL_SAFE_NO_PAD
        .decode(token)
        .map_err(|_| "calendar_source_unknown")?;
    if raw.len() > MAX_TOKEN {
        return Err("calendar_source_unknown");
    }
    let value: Value = serde_json::from_slice(&raw).map_err(|_| "calendar_source_unknown")?;
    let parts = value.as_array().filter(|parts| parts.len() == 3).ok_or("calendar_source_unknown")?;
    if parts[0].as_u64() != Some(TOKEN_VERSION) || parts[1].as_str() != Some(account) {
        return Err("calendar_source_unknown");
    }
    bounded_id(parts[2].as_str().ok_or("calendar_source_unknown")?)
        .map(str::to_owned)
        .map_err(|_| "calendar_source_unknown")
}

fn event_token(source: &str, remote: &str) -> Result<String, &'static str> {
    let raw = serde_json::to_vec(&json!([TOKEN_VERSION, source, bounded_id(remote)?]))
        .map_err(|_| "jmap_invalid_response")?;
    let token = URL_SAFE_NO_PAD.encode(raw);
    if token.len() > MAX_TOKEN * 2 {
        return Err("jmap_response_too_large");
    }
    Ok(token)
}

fn normalize_events(
    account: &str,
    source_tokens: &[String],
    remote_sources: &[String],
    events: &[Value],
    range_start: i64,
    range_end: i64,
) -> Result<Value, &'static str> {
    let mut rows = Vec::new();
    for event in events {
        validate_object(event)?;
        let remote_id = bounded_id(event["id"].as_str().ok_or("jmap_invalid_response")?)?;
        let status = clean_text(event["status"].as_str().unwrap_or(""))?.to_uppercase();
        if status == "CANCELLED" {
            continue;
        }
        let calendar_ids = event["calendarIds"].as_object().ok_or("jmap_invalid_response")?;
        if calendar_ids.len() > MAX_CALENDARS {
            return Err("jmap_response_too_large");
        }
        for (id, included) in calendar_ids {
            bounded_id(id)?;
            if !included.is_boolean() {
                return Err("jmap_invalid_response");
            }
        }
        let Some(source_index) = remote_sources.iter().position(|source| {
            calendar_ids.get(source).and_then(Value::as_bool) == Some(true)
        }) else {
            continue;
        };
        let utc_start = utc_ms(&event["utcStart"])?;
        let utc_end = utc_ms(&event["utcEnd"])?;
        if utc_end <= utc_start {
            return Err("jmap_invalid_response");
        }
        let all_day = event["showWithoutTime"] == true;
        let (start, end, tzid) = if all_day {
            let (start, end) = all_day_times(event)?;
            (start, end, String::new())
        } else {
            (utc_start, utc_end, clean_text(event["timeZone"].as_str().unwrap_or(""))?)
        };
        if start >= range_end || end <= range_start {
            continue;
        }
        let (organizer, attendees) = people(&event["participants"])?;
        let source_id = &source_tokens[source_index];
        let title = clean_text(event["title"].as_str().unwrap_or(""))?;
        let summary = if title.is_empty() { "Untitled event".to_owned() } else { title };
        rows.push(json!({
            "id": event_token(source_id, remote_id)?,
            "sourceId": source_id,
            "uid": clean_text(event["uid"].as_str().unwrap_or(remote_id))?,
            "summary": summary,
            "description": clean_text(event["description"].as_str().unwrap_or(""))?,
            "location": location(event)?,
            "meetLink": "",
            "status": status,
            "organizer": organizer,
            "attendees": attendees,
            "start": {"ms": start, "allDay": all_day, "tzid": tzid, "resolved": true},
            "end": {"ms": end, "allDay": all_day, "tzid": tzid, "resolved": true}
        }));
    }
    rows.sort_by(|left, right| {
        left["start"]["ms"].as_i64().cmp(&right["start"]["ms"].as_i64())
            .then_with(|| left["summary"].as_str().cmp(&right["summary"].as_str()))
            .then_with(|| left["id"].as_str().cmp(&right["id"].as_str()))
    });
    let _ = account; // Identity is carried by the source token, never emitted separately per event.
    Ok(json!({"events": rows}))
}

fn location(event: &Value) -> Result<String, &'static str> {
    let value = &event["locations"];
    if value.is_null() {
        return match event["location"].as_str() {
            Some(legacy) => clean_text(legacy),
            None if event["location"].is_null() => Ok(String::new()),
            None => Err("jmap_invalid_response"),
        };
    }
    let Some(locations) = value.as_object() else {
        return Err("jmap_invalid_response");
    };
    if locations.len() > MAX_LOCATIONS {
        return Err("jmap_response_too_large");
    }
    for (id, location) in locations {
        bounded_id(id)?;
        validate_object(location)?;
        let name = clean_text(location["name"].as_str().unwrap_or(""))?;
        if !name.is_empty() {
            return Ok(name);
        }
    }
    Ok(String::new())
}

fn people(value: &Value) -> Result<(String, Vec<String>), &'static str> {
    let Some(participants) = value.as_object() else {
        return if value.is_null() { Ok((String::new(), Vec::new())) } else { Err("jmap_invalid_response") };
    };
    if participants.len() > MAX_PARTICIPANTS {
        return Err("jmap_response_too_large");
    }
    let mut organizer = String::new();
    let mut attendees = Vec::new();
    for (id, participant) in participants {
        bounded_id(id)?;
        validate_object(participant)?;
        let email = clean_text(participant["email"].as_str().unwrap_or(""))?;
        let name = clean_text(participant["name"].as_str().unwrap_or(""))?;
        let display = if email.is_empty() { name } else { email };
        if display.is_empty() {
            continue;
        }
        let owner = participant["roles"]
            .as_object()
            .map(|roles| {
                if roles.len() > MAX_OBJECT_FIELDS { return Err("jmap_response_too_large"); }
                Ok(roles["owner"] == true)
            })
            .transpose()?
            .unwrap_or(false);
        if owner && organizer.is_empty() {
            organizer = display;
        } else {
            attendees.push(display);
        }
    }
    Ok((organizer, attendees))
}

fn all_day_times(event: &Value) -> Result<(i64, i64), &'static str> {
    let start_text = event["start"].as_str().ok_or("jmap_invalid_response")?;
    if start_text.len() > 64 || start_text.chars().any(char::is_control) {
        return Err("jmap_invalid_response");
    }
    let start_local = NaiveDateTime::parse_from_str(start_text, "%Y-%m-%dT%H:%M:%S%.f")
        .map_err(|_| "jmap_invalid_response")?;
    if start_local.time() != chrono::NaiveTime::default() {
        return Err("jmap_invalid_response");
    }
    let days = calendar_days(event["duration"].as_str().ok_or("jmap_invalid_response")?)?;
    let end_day = start_local.date()
        .checked_add_signed(chrono::Duration::days(days))
        .ok_or("jmap_invalid_response")?;
    let start = local_midnight(start_local.date())?;
    let end = local_midnight(end_day)?;
    if end <= start { return Err("jmap_invalid_response"); }
    Ok((start, end))
}

fn calendar_days(duration: &str) -> Result<i64, &'static str> {
    let digits = duration.strip_prefix('P')
        .and_then(|value| value.strip_suffix('D'))
        .ok_or("jmap_invalid_response")?;
    if digits.is_empty() || digits.len() > 6 || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("jmap_invalid_response");
    }
    let days = digits.parse::<i64>().map_err(|_| "jmap_invalid_response")?;
    if !(1..=366_000).contains(&days) { return Err("jmap_invalid_response"); }
    Ok(days)
}

fn local_midnight(day: NaiveDate) -> Result<i64, &'static str> {
    let midnight = day.and_hms_opt(0, 0, 0).ok_or("jmap_invalid_response")?;
    Local.from_local_datetime(&midnight)
        .earliest()
        .map(|value| value.timestamp_millis())
        .ok_or("jmap_invalid_response")
}

fn utc_local(ms: i64) -> Result<String, &'static str> {
    Utc.timestamp_millis_opt(ms)
        .single()
        .map(|value| value.format("%Y-%m-%dT%H:%M:%S%.3f").to_string())
        .ok_or("invalid_params")
}

fn utc_ms(value: &Value) -> Result<i64, &'static str> {
    let text = value.as_str().ok_or("jmap_invalid_response")?;
    if text.len() > 64 || text.chars().any(char::is_control) {
        return Err("jmap_invalid_response");
    }
    DateTime::parse_from_rfc3339(text)
        .map(|value| value.timestamp_millis())
        .map_err(|_| "jmap_invalid_response")
}

fn validate_object(value: &Value) -> Result<(), &'static str> {
    let object = value.as_object().ok_or("jmap_invalid_response")?;
    if object.len() > MAX_OBJECT_FIELDS {
        return Err("jmap_response_too_large");
    }
    Ok(())
}

fn clean_text(value: &str) -> Result<String, &'static str> {
    if value.len() > MAX_TEXT_BYTES {
        return Err("jmap_response_too_large");
    }
    Ok(value
        .chars()
        .filter(|character| !character.is_control())
        .take(MAX_TEXT_CHARS)
        .collect())
}

fn bounded_id(value: &str) -> Result<&str, &'static str> {
    if value.is_empty()
        || value.len() > MAX_ID
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

fn usize_value(value: &Value) -> Result<usize, &'static str> {
    value
        .as_u64()
        .and_then(|value| usize::try_from(value).ok())
        .ok_or("jmap_invalid_response")
}

fn charge_json_bytes(used: &mut usize, value: &Value) -> Result<(), &'static str> {
    let size = serde_json::to_vec(value).map_err(|_| "jmap_invalid_response")?.len();
    *used = used.checked_add(size).ok_or("jmap_response_too_large")?;
    if *used > super::MAX_BODY {
        return Err("jmap_response_too_large");
    }
    Ok(())
}

fn note<T>(context: &mailbox::Context, result: &Result<T, &'static str>) {
    if matches!(result, Err("jmap_unauthorized")) {
        context.rejected.store(true, std::sync::atomic::Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(document: Value) -> mailbox::Snapshot {
        mailbox::Snapshot {
            document,
            boxes: vec![], credential: json!({}), address: "user@example.test".into(),
            account: "mail-account".into(), roles: json!({}),
            slots: Arc::new(tokio::sync::Semaphore::new(1)),
            uploads: Arc::new(tokio::sync::Semaphore::new(1)),
        }
    }

    #[test]
    fn capability_uses_calendar_primary_account() {
        let value = snapshot(json!({
            "capabilities": {CALENDARS:{}},
            "primaryAccounts": {CALENDARS:"calendar-account"},
            "accounts": {"calendar-account":{"accountCapabilities":{CALENDARS:{}}}}
        }));
        assert_eq!(calendar_account(&value), Ok("calendar-account"));
        assert_eq!(calendar_account(&snapshot(json!({}))), Err("calendar_not_supported"));
    }

    #[test]
    fn source_tokens_are_versioned_reversible_and_account_scoped() {
        let token = encode_token("jmap:a@example.test", "remote/with:delimiters").unwrap();
        assert_eq!(decode_token(&token, "jmap:a@example.test"), Ok("remote/with:delimiters".into()));
        assert_eq!(decode_token(&token, "jmap:b@example.test"), Err("calendar_source_unknown"));
        assert_eq!(decode_token("not base64", "jmap:a@example.test"), Err("calendar_source_unknown"));
    }

    #[test]
    fn sources_include_only_readable_default_or_subscribed_calendars() {
        let calendars = json!([
            {"id":"hidden","name":"Hidden","isDefault":true,"isSubscribed":true,"isVisible":true,"myRights":{"mayReadItems":false}},
            {"id":"shared","name":"Shared","isDefault":false,"isSubscribed":false,"isVisible":true,"myRights":{"mayReadItems":true}},
            {"id":"main","name":"Main\nCalendar","isDefault":true,"isSubscribed":true,"isVisible":true,"myRights":{"mayReadItems":true}}
        ]);
        let value = sources_of("jmap:user@example.test", calendars.as_array().unwrap()).unwrap();
        assert_eq!(value["sources"].as_array().unwrap().len(), 1);
        let source = &value["sources"][0];
        assert_eq!(source["accountId"], "jmap:user@example.test");
        assert_eq!(source["kind"], "account");
        assert_eq!(source["name"], "MainCalendar");
        assert_eq!(source["enabled"], true);
        assert_eq!(source["readOnly"], true);
        assert_eq!(decode_token(source["id"].as_str().unwrap(), "jmap:user@example.test"), Ok("main".into()));
    }

    #[test]
    fn query_shape_has_utc_expansion_without_sort_or_calendar_filter() {
        let shape = json!({
            "accountId":"calendar-account", "timeZone":"Etc/UTC", "expandRecurrences":true,
            "filter":{"after":utc_local(0).unwrap(),"before":utc_local(1000).unwrap()},
            "position":0,"limit":MAX_EVENTS,"calculateTotal":true
        });
        assert_eq!(shape["filter"], json!({"after":"1970-01-01T00:00:00.000","before":"1970-01-01T00:00:01.000"}));
        assert!(shape.get("sort").is_none());
        assert!(shape["filter"].get("inCalendar").is_none());
    }

    #[test]
    fn normalizes_exact_utc_times_and_assigns_first_selected_source() {
        let a = encode_token("jmap:user@example.test", "a").unwrap();
        let b = encode_token("jmap:user@example.test", "b").unwrap();
        let events = vec![json!({
            "id":"event-1","uid":"uid-1","title":"Meeting","description":"Notes",
            "locations":{"l":{"name":"Room 1"}},
            "participants":{"owner":{"email":"owner@example.test","roles":{"owner":true}},"guest":{"email":"guest@example.test"}},
            "status":"confirmed","calendarIds":{"a":true,"b":true},"showWithoutTime":false,
            "timeZone":"Europe/Berlin","utcStart":"2026-01-02T10:00:00Z","utcEnd":"2026-01-02T11:00:00Z"
        })];
        let value = normalize_events("jmap:user@example.test", &[b.clone(), a], &["b".into(), "a".into()], &events, 0, 2_000_000_000_000).unwrap();
        let event = &value["events"][0];
        assert_eq!(event["sourceId"], b);
        assert_eq!(event["start"]["ms"], 1767348000000_i64);
        assert_eq!(event["end"]["ms"], 1767351600000_i64);
        assert_eq!(event["organizer"], "owner@example.test");
        assert_eq!(event["attendees"], json!(["guest@example.test"]));
        assert_eq!(event["location"], "Room 1");
        assert!(event.get("calendarIds").is_none());
    }

    #[test]
    fn event_source_selection_skips_requested_calendars_without_membership() {
        let absent = encode_token("jmap:user@example.test", "absent").unwrap();
        let present = encode_token("jmap:user@example.test", "present").unwrap();
        let events = vec![json!({
            "id":"event-1","calendarIds":{"present":true},
            "utcStart":"2026-01-02T10:00:00Z","utcEnd":"2026-01-02T11:00:00Z"
        })];
        let value = normalize_events(
            "jmap:user@example.test", &[absent, present.clone()],
            &["absent".into(), "present".into()], &events, 0, 2_000_000_000_000,
        ).unwrap();
        assert_eq!(value["events"][0]["sourceId"], present);
    }

    #[test]
    fn malformed_times_fail_closed_and_cancelled_or_nonoverlapping_are_filtered() {
        let token = encode_token("jmap:user@example.test", "a").unwrap();
        let malformed = vec![json!({"id":"e","calendarIds":{"a":true},"utcStart":"local","utcEnd":"2026-01-01T01:00:00Z"})];
        assert_eq!(normalize_events("jmap:user@example.test", std::slice::from_ref(&token), &["a".into()], &malformed, 0, i64::MAX), Err("jmap_invalid_response"));
        let filtered = vec![
            json!({"id":"c","status":"cancelled","calendarIds":{"a":true},"utcStart":"2026-01-01T00:00:00Z","utcEnd":"2026-01-01T01:00:00Z"}),
            json!({"id":"o","calendarIds":{"a":true},"utcStart":"2025-01-01T00:00:00Z","utcEnd":"2025-01-01T01:00:00Z"})
        ];
        assert_eq!(normalize_events("jmap:user@example.test", &[token], &["a".into()], &filtered, 1767225600000, 1767312000000).unwrap(), json!({"events":[]}));
    }

    struct Peer(std::process::Child);
    impl Drop for Peer {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    async fn fixture_session_for(scenario: &str) -> (Peer, Session, String) {
        use std::io::{BufRead, BufReader};
        let mut command = std::process::Command::new("python3");
        command.arg(concat!(env!("CARGO_MANIFEST_DIR"), "/src/providers/jmap/mailbox_tls_test.py"));
        if scenario != "default" { command.arg(scenario); }
        let mut peer = Peer(command.stdout(std::process::Stdio::piped()).spawn().unwrap());
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
                CALENDARS: {}
            },
            "accounts": {
                "mail-account": {"accountCapabilities": {mailbox::MAIL: {"emailQuerySortOptions":["receivedAt"]}}},
                "calendar-account": {"accountCapabilities": {CALENDARS: {}}}
            },
            "primaryAccounts": {
                mailbox::MAIL: "mail-account",
                CALENDARS: "calendar-account"
            }
        });
        session.install_snapshot_for_test(
            "jmap:user@example.test", document, vec![],
            json!({"scheme":"basic","username":"user","secret":"synthetic-calendar-secret"}),
            "user@example.test",
        ).await.unwrap();
        (peer, session, "jmap:user@example.test".to_owned())
    }

    #[tokio::test]
    async fn reads_sources_and_expanded_events_through_native_transport() {
        let (_peer, session, account) = fixture_session_for("default").await;
        let sources = session.calendar_sources(&account).await.unwrap();
        let rows = sources["sources"].as_array().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["name"], "Personal");
        assert_eq!(rows[0]["readOnly"], true);
        let source = rows[0]["id"].as_str().unwrap().to_owned();
        let events = session.calendar_events(
            &account, std::slice::from_ref(&source),
            1_790_035_200_000, 1_790_294_400_000,
        ).await.unwrap();
        assert_eq!(events["events"].as_array().unwrap().len(), 2);
        assert_eq!(events["events"][0]["sourceId"], source);
        assert_eq!(events["events"][0]["location"], "Room 1");
        assert_eq!(events["events"][1]["start"]["allDay"], true);
        let all_day_start = events["events"][1]["start"]["ms"].as_i64().unwrap();
        let all_day_end = events["events"][1]["end"]["ms"].as_i64().unwrap();
        let start_local = Local.timestamp_millis_opt(all_day_start).single().unwrap();
        let end_local = Local.timestamp_millis_opt(all_day_end).single().unwrap();
        assert_eq!(start_local.date_naive(), NaiveDate::from_ymd_opt(2026, 9, 23).unwrap());
        assert_eq!(end_local.date_naive(), NaiveDate::from_ymd_opt(2026, 9, 24).unwrap());
        assert_eq!(start_local.time(), chrono::NaiveTime::default());
        assert_eq!(end_local.time(), chrono::NaiveTime::default());
        assert!(!events.to_string().contains("synthetic-calendar-secret"));
    }

    #[tokio::test]
    async fn rejects_duplicate_calendar_sources() {
        let (_peer, session, account) = fixture_session_for("calendar-source-duplicate").await;
        assert_eq!(session.calendar_sources(&account).await, Err("jmap_invalid_response"));
    }

    #[tokio::test]
    async fn rejects_inconsistent_or_nonprogressing_calendar_queries() {
        for scenario in [
            "calendar-query-stall", "calendar-query-duplicate",
            "calendar-query-wrong-position", "calendar-query-state-change",
        ] {
            let (_peer, session, account) = fixture_session_for(scenario).await;
            let sources = session.calendar_sources(&account).await.unwrap();
            let source = sources["sources"][0]["id"].as_str().unwrap().to_owned();
            assert_eq!(
                session.calendar_events(&account, &[source], 1_790_035_200_000, 1_790_294_400_000).await,
                Err("jmap_invalid_response"),
                "{scenario} must fail closed"
            );
        }
        let (_peer, session, account) = fixture_session_for("calendar-query-too-many").await;
        let sources = session.calendar_sources(&account).await.unwrap();
        let source = sources["sources"][0]["id"].as_str().unwrap().to_owned();
        assert_eq!(
            session.calendar_events(&account, &[source], 1_790_035_200_000, 1_790_294_400_000).await,
            Err("calendar_too_many")
        );
    }

    #[tokio::test]
    async fn rejects_missing_duplicate_or_unsolicited_calendar_events() {
        for scenario in ["calendar-get-missing", "calendar-get-duplicate", "calendar-get-unsolicited"] {
            let (_peer, session, account) = fixture_session_for(scenario).await;
            let sources = session.calendar_sources(&account).await.unwrap();
            let source = sources["sources"][0]["id"].as_str().unwrap().to_owned();
            assert_eq!(
                session.calendar_events(&account, &[source], 1_790_035_200_000, 1_790_294_400_000).await,
                Err("jmap_invalid_response"),
                "{scenario} must fail closed"
            );
        }
    }
}
