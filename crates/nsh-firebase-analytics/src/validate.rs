//! Client-side validation, mirroring React Native Firebase's
//! `packages/analytics/lib/index.ts` (`ValidEventName`,
//! `ReservedEventNamePrefixes`, `ReservedEventNames`) as recorded in
//! docs/research.md §1.6. The same rules guard the native-tier path, so
//! both tiers reject a bad name before anything leaves the process.

use serde_json::Value as JsonValue;

use nsh_firebase_core::{FirebaseError, error::INVALID_ARGUMENT};

/// The names the native pipeline owns; logging one is refused.
const RESERVED_EVENT_NAMES: [&str; 30] = [
    "ad_activeview",
    "ad_click",
    "ad_exposure",
    "ad_query",
    "ad_reward",
    "adunit_exposure",
    "app_background",
    "app_clear_data",
    "app_remove",
    "app_store_refund",
    "app_store_subscription_cancel",
    "app_store_subscription_convert",
    "app_store_subscription_renew",
    "app_update",
    "app_upgrade",
    "dynamic_link_app_open",
    "dynamic_link_app_update",
    "dynamic_link_first_open",
    "error",
    "first_open",
    "first_visit",
    "in_app_purchase",
    "notification_dismiss",
    "notification_foreground",
    "notification_open",
    "notification_receive",
    "os_update",
    "session_start",
    "session_start_with_rollout",
    "user_engagement",
];

const RESERVED_EVENT_NAME_PREFIXES: [&str; 3] = ["firebase_", "google_", "ga_"];

/// RNFB's `ValidEventName`: an alphabetic first character, then 1-40
/// alphanumeric-or-underscore characters in total.
fn is_valid_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphabetic() => {}
        _ => return false,
    }
    let rest = chars.count();
    rest < 40
        && name[1..]
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn invalid(message: impl Into<String>) -> FirebaseError {
    FirebaseError::new(INVALID_ARGUMENT, message)
}

/// An event name the queue accepts.
pub fn validate_event_name(name: &str) -> Result<(), FirebaseError> {
    if RESERVED_EVENT_NAMES.contains(&name) {
        return Err(invalid(format!(
            "logEvent: the event name '{name}' is reserved and can not be used"
        )));
    }
    if RESERVED_EVENT_NAME_PREFIXES
        .iter()
        .any(|prefix| name.starts_with(prefix))
    {
        return Err(invalid(format!(
            "logEvent: the event name '{name}' uses a reserved prefix and can not be used"
        )));
    }
    if !is_valid_identifier(name) {
        return Err(invalid(format!(
            "logEvent: invalid event name '{name}'. Names must start with an alphabetic \
             character and contain 1 to 40 alphanumeric characters or underscores"
        )));
    }
    Ok(())
}

/// GA4 params are flat: primitive values only, keys shaped like event
/// names. Nested objects, arrays and null are refused here rather than
/// silently flattened by the endpoint.
pub fn validate_params(params: Option<&JsonValue>) -> Result<(), FirebaseError> {
    let Some(params) = params else {
        return Ok(());
    };
    let object = params
        .as_object()
        .ok_or_else(|| invalid("logEvent: params expected an object value"))?;
    for (key, value) in object {
        if !is_valid_identifier(key) {
            return Err(invalid(format!(
                "logEvent: invalid param name '{key}'. Names must start with an alphabetic \
                 character and contain 1 to 40 alphanumeric characters or underscores"
            )));
        }
        if !value.is_string() && !value.is_number() && !value.is_boolean() {
            return Err(invalid(format!(
                "logEvent: the param '{key}' must be a string, number or boolean"
            )));
        }
    }
    Ok(())
}

/// A user property key is shaped like an event name; its value is a
/// primitive. GA4 reserves the `google_` prefix for its own properties,
/// so it is refused (the `firebase_`/`ga_` prefixes are event-name
/// rules, not user-property ones).
pub fn validate_user_property(key: &str, value: &JsonValue) -> Result<(), FirebaseError> {
    if key.starts_with("google_") {
        return Err(invalid(format!(
            "setUserProperty: the property name '{key}' uses a reserved prefix and can not be used"
        )));
    }
    if !is_valid_identifier(key) {
        return Err(invalid(format!(
            "setUserProperty: invalid property name '{key}'. Names must start with an alphabetic \
             character and contain 1 to 40 alphanumeric characters or underscores"
        )));
    }
    if !value.is_string() && !value.is_number() && !value.is_boolean() {
        return Err(invalid(format!(
            "setUserProperty: the value of '{key}' must be a string, number or boolean"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn ordinary_names_pass() {
        for name in ["level_up", "screen_view", "a", "X2_y"] {
            assert!(validate_event_name(name).is_ok(), "{name} must pass");
        }
    }

    #[test]
    fn reserved_names_and_prefixes_fail() {
        assert_eq!(
            validate_event_name("session_start").unwrap_err().code,
            INVALID_ARGUMENT
        );
        for name in ["firebase_screen", "google_foo", "ga_bar"] {
            assert!(validate_event_name(name).is_err(), "{name} must fail");
        }
    }

    #[test]
    fn malformed_names_fail() {
        for name in [
            "",
            "1start",
            "_under",
            "has space",
            "with-dash",
            &"a".repeat(41),
        ] {
            assert!(validate_event_name(name).is_err(), "{name} must fail");
        }
        assert!(validate_event_name(&"a".repeat(40)).is_ok());
    }

    #[test]
    fn params_must_be_an_object_of_primitives() {
        assert!(validate_params(None).is_ok());
        assert!(validate_params(Some(&json!({"level": 3, "hard": true, "tag": "x"}))).is_ok());
        assert!(validate_params(Some(&json!([1]))).is_err());
        assert!(validate_params(Some(&json!({"nested": {"a": 1}}))).is_err());
        assert!(validate_params(Some(&json!({"bad key": 1}))).is_err());
        assert!(validate_params(Some(&json!({"nothing": null}))).is_err());
    }

    #[test]
    fn user_property_keys_and_values_are_checked() {
        assert!(validate_user_property("difficulty", &json!("hard")).is_ok());
        assert!(validate_user_property("ga_x", &json!("hard")).is_ok());
        assert!(validate_user_property("google_secret", &json!("x")).is_err());
        assert!(validate_user_property("1bad", &json!("x")).is_err());
        assert!(validate_user_property("difficulty", &json!({"deep": 1})).is_err());
    }
}
