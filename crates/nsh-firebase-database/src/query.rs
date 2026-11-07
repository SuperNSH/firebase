//! Query params and the client-side sort: the REST filters of
//! <https://firebase.google.com/docs/database/rest/retrieve-data>
//! (`orderBy`, `startAt`, `endAt`, `equalTo`, `limitToFirst`,
//! `limitToLast`), encoded as URL query parameters. The server answers
//! unsorted; [`sort_children`] puts the children in the documented
//! order.

use serde_json::Value as JsonValue;

use nsh_firebase_core::error::{FirebaseError, INVALID_ARGUMENT};

/// `$key`: order children by their keys.
pub const ORDER_KEY: &str = "$key";
/// `$value`: order children by their values.
pub const ORDER_VALUE: &str = "$value";

/// What an `orderBy` orders by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Order {
    /// A child's value, at `key` (a `/`-separated path for nested keys).
    Child(String),
    /// The children's keys.
    Key,
    /// The children's values.
    Value,
}

impl Order {
    /// The `orderBy` string of it.
    pub fn as_param(&self) -> String {
        match self {
            Order::Child(key) => key.clone(),
            Order::Key => ORDER_KEY.to_owned(),
            Order::Value => ORDER_VALUE.to_owned(),
        }
    }

    /// Parses an `orderBy` value (`"$key"`, `"$value"`, or a child path).
    pub fn parse(order_by: &str) -> Order {
        match order_by {
            ORDER_KEY => Order::Key,
            ORDER_VALUE => Order::Value,
            key => Order::Child(key.to_owned()),
        }
    }
}

/// The query a [`crate::DatabaseReference`] carries, encoded on `get`
/// and on listeners as REST query parameters.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct QueryParams {
    pub order_by: Option<String>,
    /// Inclusive lower bound (a JSON value).
    pub start_at: Option<JsonValue>,
    /// Inclusive upper bound.
    pub end_at: Option<JsonValue>,
    /// Exactly this value.
    pub equal_to: Option<JsonValue>,
    pub limit_to_first: Option<u32>,
    pub limit_to_last: Option<u32>,
}

impl QueryParams {
    /// The filters the docs document: one `orderBy` per request, and
    /// every filter needs one; a request has one limit at most.
    pub fn validate(&self) -> Result<(), FirebaseError> {
        let filtered = self.start_at.is_some()
            || self.end_at.is_some()
            || self.equal_to.is_some()
            || self.limit_to_first.is_some()
            || self.limit_to_last.is_some();
        if filtered && self.order_by.is_none() {
            return Err(FirebaseError::new(
                INVALID_ARGUMENT,
                "a query filter needs orderBy (orderByChild, orderByKey or orderByValue)",
            ));
        }
        if self.limit_to_first.is_some() && self.limit_to_last.is_some() {
            return Err(FirebaseError::new(
                INVALID_ARGUMENT,
                "limitToFirst and limitToLast cannot combine",
            ));
        }
        Ok(())
    }

    /// The query as ordered `(name, encoded value)` pairs (the order is
    /// fixed, so a test's stub route can match the URL whole). `auth` is
    /// appended by the caller that knows the token.
    pub fn encode(&self) -> Result<Vec<(String, String)>, FirebaseError> {
        self.validate()?;
        let mut params = Vec::new();
        if let Some(order_by) = &self.order_by {
            params.push((
                "orderBy".to_owned(),
                encode_component(&JsonValue::String(order_by.clone()).to_string()),
            ));
        }
        for (name, value) in [
            ("startAt", &self.start_at),
            ("endAt", &self.end_at),
            ("equalTo", &self.equal_to),
        ] {
            if let Some(value) = value {
                params.push((name.to_owned(), encode_component(&value.to_string())));
            }
        }
        if let Some(limit) = self.limit_to_first {
            params.push(("limitToFirst".to_owned(), limit.to_string()));
        }
        if let Some(limit) = self.limit_to_last {
            params.push(("limitToLast".to_owned(), limit.to_string()));
        }
        Ok(params)
    }

    /// Whether a query is set at all.
    pub fn is_empty(&self) -> bool {
        *self == QueryParams::default()
    }
}

/// The children of an object value, in the order `order` puts them. A
/// non-object keeps as it is. The sort is stable, and object values
/// compare as equal (the docs put objects last without an inner order).
pub fn sort_children(root: &mut JsonValue, order: &Order) {
    let JsonValue::Object(entries) = root else {
        return;
    };
    let mut sorted: Vec<(String, JsonValue)> = entries
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let key_order = matches!(order, Order::Key);
    sorted.sort_by(|a, b| {
        if key_order {
            compare_keys(&a.0, &b.0)
        } else {
            let a_value = child_value(&a.1, order);
            let b_value = child_value(&b.1, order);
            compare_values(&a_value, &b_value)
        }
    });
    *entries = sorted.into_iter().collect();
}

/// The value an order reads: the child at its key (`Order::Child`), or
/// the value itself (`Order::Value`). A missing child is null, which the
/// documented order puts first.
fn child_value(value: &JsonValue, order: &Order) -> JsonValue {
    match order {
        Order::Child(key) => {
            let mut at = value;
            for part in key.split('/') {
                at = at.get(part).unwrap_or(&JsonValue::Null);
            }
            at.clone()
        }
        Order::Value => value.clone(),
        Order::Key => JsonValue::Null,
    }
}

/// The documented total order: null < false < true < numbers < strings
/// < objects; within numbers and strings, their natural order.
pub fn compare_values(a: &JsonValue, b: &JsonValue) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let rank = |value: &JsonValue| match value {
        JsonValue::Null => 0,
        JsonValue::Bool(false) => 1,
        JsonValue::Bool(true) => 2,
        JsonValue::Number(_) => 3,
        JsonValue::String(_) => 4,
        JsonValue::Array(_) | JsonValue::Object(_) => 5,
    };
    match rank(a).cmp(&rank(b)) {
        Ordering::Equal => match (a, b) {
            (JsonValue::Number(a), JsonValue::Number(b)) => a
                .as_f64()
                .partial_cmp(&b.as_f64())
                .unwrap_or(Ordering::Equal),
            (JsonValue::String(a), JsonValue::String(b)) => a.cmp(b),
            _ => Ordering::Equal,
        },
        order => order,
    }
}

/// Keys parseable as 32-bit integers come first, in numeric order; the
/// rest follow lexicographically.
fn compare_keys(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (a.parse::<i32>(), b.parse::<i32>()) {
        (Ok(a), Ok(b)) => a.cmp(&b),
        (Ok(_), Err(_)) => Ordering::Less,
        (Err(_), Ok(_)) => Ordering::Greater,
        (Err(_), Err(_)) => a.cmp(b),
    }
}

/// Percent-encodes a query parameter's value, as `encodeURIComponent`
/// does but keeping only the RFC 3986 unreserved set, so the TypeScript
/// side's URLs match character for character on everything else.
pub fn encode_component(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_query_encodes_in_a_fixed_order() {
        let query = QueryParams {
            order_by: Some("height".to_owned()),
            start_at: Some(json!(3)),
            end_at: Some(json!("pterodactyl")),
            equal_to: None,
            limit_to_first: Some(2),
            limit_to_last: None,
        };
        let encoded = query.encode().unwrap();
        let pairs: Vec<String> = encoded.iter().map(|(k, v)| format!("{k}={v}")).collect();
        assert_eq!(
            pairs.join("&"),
            "orderBy=%22height%22&startAt=3&endAt=%22pterodactyl%22&limitToFirst=2"
        );
    }

    #[test]
    fn key_and_value_orders_encode_as_the_docs_write_them() {
        let by_key = QueryParams {
            order_by: Some(ORDER_KEY.to_owned()),
            start_at: Some(json!("a")),
            ..QueryParams::default()
        };
        assert_eq!(
            by_key.encode().unwrap(),
            vec![
                ("orderBy".to_owned(), encode_component(r#""$key""#)),
                ("startAt".to_owned(), "%22a%22".to_owned())
            ]
        );
    }

    #[test]
    fn a_filter_without_orderby_is_invalid_argument() {
        let query = QueryParams {
            limit_to_first: Some(2),
            ..QueryParams::default()
        };
        let error = query.encode().unwrap_err();
        assert_eq!(error.code, INVALID_ARGUMENT);
    }

    #[test]
    fn two_limits_are_invalid_argument() {
        let query = QueryParams {
            order_by: Some(ORDER_KEY.to_owned()),
            limit_to_first: Some(2),
            limit_to_last: Some(2),
            ..QueryParams::default()
        };
        assert_eq!(query.encode().unwrap_err().code, INVALID_ARGUMENT);
    }

    #[test]
    fn an_empty_query_encodes_nothing() {
        let query = QueryParams::default();
        assert_eq!(query.encode().unwrap(), Vec::<(String, String)>::new());
        assert!(query.is_empty());
    }

    #[test]
    fn children_sort_null_false_true_number_string_object() {
        let value = json!({
            "an_object": {"deep": true},
            "a_string": "zeta",
            "a_number": 42,
            "true_value": true,
            "a_null": null,
            "a_false": false,
        });
        let mut sorted = value;
        sort_children(&mut sorted, &Order::Value);
        let order: Vec<&str> = sorted
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            order,
            [
                "a_null",
                "a_false",
                "true_value",
                "a_number",
                "a_string",
                "an_object"
            ]
        );
    }

    #[test]
    fn integer_keys_sort_first_and_numerically() {
        let mut keys = json!({
            "zeta": 1,
            "20": 1,
            "alpha": 1,
            "3": 1,
        });
        sort_children(&mut keys, &Order::Key);
        let order: Vec<&str> = keys
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(order, ["3", "20", "alpha", "zeta"]);
    }

    #[test]
    fn a_child_order_reads_a_nested_path_and_sorts_numbers() {
        let value = json!({
            "b": {"meta": {"score": 9}},
            "a": {"meta": {"score": 1}},
            "c": {},
        });
        let mut sorted = value;
        sort_children(&mut sorted, &Order::Child("meta/score".to_owned()));
        let order: Vec<&str> = sorted
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        // `c` has no meta/score: null sorts first.
        assert_eq!(order, ["c", "a", "b"]);
    }

    #[test]
    fn encoding_matches_javascripts_encodeuricomponent_on_shared_characters() {
        assert_eq!(encode_component(r#""$key""#), "%22%24key%22");
        assert_eq!(encode_component("a b&c"), "a%20b%26c");
        assert_eq!(
            encode_component("safe-_育苗.~"),
            "safe-_%E8%82%B2%E8%8B%97.~"
        );
    }

    #[test]
    fn order_parses_the_dollar_forms() {
        assert_eq!(Order::parse("$key"), Order::Key);
        assert_eq!(Order::parse("$value"), Order::Value);
        assert_eq!(
            Order::parse("meta/score"),
            Order::Child("meta/score".to_owned())
        );
        assert_eq!(Order::Child("meta".to_owned()).as_param(), "meta");
        assert_eq!(Order::Key.as_param(), "$key");
    }
}
