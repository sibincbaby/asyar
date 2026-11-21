//! Cross-platform calendar and reminders abstraction.
//!
//! - Linux: local `.ics` files (`linux::calendar`, via `ics`).
//!
//! Platforms without a backend return [`PlatformError::Unsupported`] rather
//! than an empty result, so callers never mistake "no backend" for "nothing
//! scheduled".
//!
//! All timestamps are Unix epoch milliseconds. Input validation lives here, in
//! front of the backends, so every platform rejects the same bad input.

use crate::error::PlatformError;
use serde::{Deserialize, Serialize};

/// EventKit only evaluates event predicates over a window of about four years.
pub const MAX_EVENT_RANGE_MS: i64 = 4 * 366 * 24 * 60 * 60 * 1000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarSource {
    pub id: String,
    pub title: String,
    /// The account the calendar belongs to, e.g. `iCloud`.
    pub source_title: Option<String>,
    pub read_only: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarEvent {
    pub id: String,
    pub calendar_id: String,
    pub calendar_title: String,
    pub title: String,
    pub start_at: i64,
    pub end_at: i64,
    pub is_all_day: bool,
    pub location: Option<String>,
    pub notes: Option<String>,
    pub url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateEventInput {
    /// Defaults to the user's default calendar for new events.
    pub calendar_id: Option<String>,
    pub title: String,
    pub start_at: i64,
    pub end_at: i64,
    pub is_all_day: Option<bool>,
    pub location: Option<String>,
    pub notes: Option<String>,
    pub url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReminderList {
    pub id: String,
    pub title: String,
    pub source_title: Option<String>,
    pub read_only: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReminderItem {
    pub id: String,
    pub list_id: String,
    pub list_title: String,
    pub title: String,
    pub is_completed: bool,
    pub due_at: Option<i64>,
    /// `0` none, `1`–`4` high, `5` medium, `6`–`9` low (EventKit semantics).
    pub priority: i32,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateReminderInput {
    /// Defaults to the user's default reminder list.
    pub list_id: Option<String>,
    pub title: String,
    pub due_at: Option<i64>,
    pub priority: Option<i32>,
    pub notes: Option<String>,
}

#[cfg(target_os = "linux")]
use crate::linux::calendar as backend;

fn invalid(msg: &str) -> PlatformError {
    PlatformError::InvalidInput(msg.to_string())
}

pub(crate) fn validate_event_range(start_at: i64, end_at: i64) -> Result<(), PlatformError> {
    if start_at < 0 || end_at < 0 {
        return Err(invalid("timestamps must not be negative"));
    }
    if end_at < start_at {
        return Err(invalid("endAt must not be before startAt"));
    }
    if end_at - start_at > MAX_EVENT_RANGE_MS {
        return Err(invalid("range must not exceed four years"));
    }
    Ok(())
}

pub(crate) fn validate_create_event(input: &CreateEventInput) -> Result<(), PlatformError> {
    if input.title.trim().is_empty() {
        return Err(invalid("title must not be empty"));
    }
    if input.start_at < 0 || input.end_at < 0 {
        return Err(invalid("timestamps must not be negative"));
    }
    if input.end_at < input.start_at {
        return Err(invalid("endAt must not be before startAt"));
    }
    if input
        .url
        .as_deref()
        .is_some_and(|u| u.chars().any(char::is_control))
    {
        return Err(invalid("url must not contain control characters"));
    }
    Ok(())
}

pub(crate) fn validate_create_reminder(input: &CreateReminderInput) -> Result<(), PlatformError> {
    if input.title.trim().is_empty() {
        return Err(invalid("title must not be empty"));
    }
    if matches!(input.due_at, Some(due) if due < 0) {
        return Err(invalid("dueAt must not be negative"));
    }
    if matches!(input.priority, Some(p) if !(0..=9).contains(&p)) {
        return Err(invalid("priority must be between 0 and 9"));
    }
    Ok(())
}

/// Every calendar the user has, including read-only subscribed ones.
pub fn list_calendars() -> Result<Vec<CalendarSource>, PlatformError> {
    backend::list_calendars()
}

/// Events overlapping `[start_at, end_at]`, sorted by start time. `calendar_ids`
/// of `None` searches every calendar.
pub fn list_events(
    start_at: i64,
    end_at: i64,
    calendar_ids: Option<&[String]>,
) -> Result<Vec<CalendarEvent>, PlatformError> {
    validate_event_range(start_at, end_at)?;
    backend::list_events(start_at, end_at, calendar_ids)
}

pub fn create_event(input: CreateEventInput) -> Result<CalendarEvent, PlatformError> {
    validate_create_event(&input)?;
    backend::create_event(input)
}

pub fn list_reminder_lists() -> Result<Vec<ReminderList>, PlatformError> {
    backend::list_reminder_lists()
}

/// Reminders in `list_ids` (every list when `None`). Completed reminders are
/// only returned when `include_completed` is set.
pub fn list_reminders(
    list_ids: Option<&[String]>,
    include_completed: bool,
) -> Result<Vec<ReminderItem>, PlatformError> {
    backend::list_reminders(list_ids, include_completed)
}

pub fn create_reminder(input: CreateReminderInput) -> Result<ReminderItem, PlatformError> {
    validate_create_reminder(&input)?;
    backend::create_reminder(input)
}

pub fn set_reminder_completed(id: &str, completed: bool) -> Result<ReminderItem, PlatformError> {
    if id.trim().is_empty() {
        return Err(invalid("reminder id must not be empty"));
    }
    backend::set_reminder_completed(id, completed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(title: &str, start_at: i64, end_at: i64) -> CreateEventInput {
        CreateEventInput {
            calendar_id: None,
            title: title.into(),
            start_at,
            end_at,
            is_all_day: None,
            location: None,
            notes: None,
            url: None,
        }
    }

    fn reminder(title: &str) -> CreateReminderInput {
        CreateReminderInput {
            list_id: None,
            title: title.into(),
            due_at: None,
            priority: None,
            notes: None,
        }
    }

    #[test]
    fn event_range_rejects_end_before_start() {
        assert!(validate_event_range(2_000, 1_000).is_err());
        assert!(validate_event_range(1_000, 1_000).is_ok());
    }

    #[test]
    fn event_range_rejects_negative_and_oversized() {
        assert!(validate_event_range(-1, 5).is_err());
        assert!(validate_event_range(0, MAX_EVENT_RANGE_MS + 1).is_err());
        assert!(validate_event_range(0, MAX_EVENT_RANGE_MS).is_ok());
    }

    #[test]
    fn create_event_rejects_blank_title() {
        assert!(validate_create_event(&event("", 0, 1)).is_err());
        assert!(validate_create_event(&event("   ", 0, 1)).is_err());
        assert!(validate_create_event(&event("Standup", 0, 1)).is_ok());
    }

    #[test]
    fn create_event_rejects_end_before_start() {
        assert!(validate_create_event(&event("Standup", 10, 5)).is_err());
    }

    #[test]
    fn create_event_rejects_control_characters_in_url() {
        let mut e = event("Standup", 0, 1);
        e.url = Some("https://a.example\nEND:VEVENT".into());
        assert!(validate_create_event(&e).is_err());
        e.url = Some("https://a.example/ok".into());
        assert!(validate_create_event(&e).is_ok());
    }

    #[test]
    fn create_reminder_validates_title_due_and_priority() {
        assert!(validate_create_reminder(&reminder(" ")).is_err());
        assert!(validate_create_reminder(&reminder("Milk")).is_ok());

        let mut r = reminder("Milk");
        r.due_at = Some(-5);
        assert!(validate_create_reminder(&r).is_err());

        r.due_at = Some(5);
        r.priority = Some(10);
        assert!(validate_create_reminder(&r).is_err());
        r.priority = Some(-1);
        assert!(validate_create_reminder(&r).is_err());
        r.priority = Some(9);
        assert!(validate_create_reminder(&r).is_ok());
    }

    #[test]
    fn public_api_validates_before_touching_the_os() {
        // Rejected by validation, so no permission prompt or EventKit access.
        assert!(list_events(10, 5, None).is_err());
        assert!(create_event(event("", 0, 1)).is_err());
        assert!(create_reminder(reminder("")).is_err());
        assert!(set_reminder_completed("  ", true).is_err());
    }

    #[test]
    fn camel_case_wire_shape() {
        let json = serde_json::json!({
            "title": "Lunch",
            "startAt": 1,
            "endAt": 2,
            "isAllDay": true,
            "calendarId": "abc"
        });
        let parsed: CreateEventInput = serde_json::from_value(json).unwrap();
        assert_eq!(parsed.calendar_id.as_deref(), Some("abc"));
        assert_eq!(parsed.is_all_day, Some(true));
    }
}
