//! Parsing and validation of a Remote Config template: the REST
//! `RemoteConfig` resource shape (`conditions`, `parameters`,
//! `parameterGroups`, `version`), read from the v1 discovery document of
//! `firebaseremoteconfig.googleapis.com`. A parameter value is a oneof:
//! `{value}` is the only shape resolvable without the backend;
//! `useInAppDefault` and the dynamic personalization/experiment/rollout
//! values leave the parameter unset for the client.

use std::collections::BTreeMap;

use serde_json::{Map, Value as JsonValue};

use nsh_firebase_core::FirebaseError;

use crate::codes::INVALID_TEMPLATE;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteConfigCondition {
    pub name: String,
    pub expression: String,
    pub tag_color: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteConfigParameter {
    /// The `{value}` string of `defaultValue`; `None` for
    /// `useInAppDefault`, a dynamic value, or no default at all.
    pub default_value: Option<String>,
    /// Condition name -> `{value}` string. Recorded, never evaluated:
    /// the condition expression language needs the backend's instance
    /// context, so the pure tier resolves the default value only.
    pub conditional_values: BTreeMap<String, String>,
    pub description: Option<String>,
    pub value_type: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct RemoteConfigTemplate {
    /// Descending priority, unique names, as the REST docs define.
    pub conditions: Vec<RemoteConfigCondition>,
    /// Top-level parameters plus every parameter group's, merged (the
    /// REST rule: a parameter appears once per template).
    pub parameters: BTreeMap<String, RemoteConfigParameter>,
    pub version: Option<JsonValue>,
}

impl RemoteConfigTemplate {
    /// The values the template resolves to for a client that evaluates
    /// no conditions: the resource's own rule — with no true condition
    /// the default value stands, and unset defaults are omitted.
    pub fn resolved_values(&self) -> BTreeMap<String, String> {
        self.parameters
            .iter()
            .filter_map(|(key, parameter)| {
                parameter
                    .default_value
                    .clone()
                    .map(|value| (key.clone(), value))
            })
            .collect()
    }
}

fn invalid(message: impl Into<String>) -> FirebaseError {
    FirebaseError::new(INVALID_TEMPLATE, message)
}

fn as_object<'a>(
    value: &'a JsonValue,
    what: &str,
) -> Result<&'a Map<String, JsonValue>, FirebaseError> {
    value
        .as_object()
        .ok_or_else(|| invalid(format!("{what} is not an object")))
}

fn optional_string(
    object: &Map<String, JsonValue>,
    key: &str,
    what: &str,
) -> Result<Option<String>, FirebaseError> {
    match object.get(key) {
        Some(JsonValue::String(text)) => Ok(Some(text.clone())),
        Some(JsonValue::Null) | None => Ok(None),
        Some(_) => Err(invalid(format!("{what}.{key} is not a string"))),
    }
}

fn required_string(
    object: &Map<String, JsonValue>,
    key: &str,
    what: &str,
) -> Result<String, FirebaseError> {
    match object.get(key) {
        Some(JsonValue::String(text)) if !text.is_empty() => Ok(text.clone()),
        Some(JsonValue::String(_)) => Err(invalid(format!("{what}.{key} is empty"))),
        _ => Err(invalid(format!("{what}.{key} is missing or not a string"))),
    }
}

/// One `RemoteConfigParameterValue`: `{value}` resolves to its string;
/// `useInAppDefault` and the dynamic value objects resolve to none.
fn parameter_value(value: &JsonValue, what: &str) -> Result<Option<String>, FirebaseError> {
    let value = as_object(value, what)?;
    if let Some(inner) = value.get("value") {
        let text = inner
            .as_str()
            .ok_or_else(|| invalid(format!("{what}.value is not a string")))?;
        return Ok(Some(text.to_owned()));
    }
    if value.contains_key("useInAppDefault") {
        if !value
            .get("useInAppDefault")
            .is_some_and(|flag| flag.is_boolean())
        {
            return Err(invalid(format!("{what}.useInAppDefault is not a boolean")));
        }
        return Ok(None);
    }
    let dynamic = value.contains_key("personalizationValue")
        || value.contains_key("experimentValue")
        || value.contains_key("rolloutValue");
    if dynamic {
        return Ok(None);
    }
    // An empty object sets no value option: the parameter stays unset.
    if value.is_empty() {
        return Ok(None);
    }
    Err(invalid(format!("{what} sets no known value option")))
}

fn parse_parameter(value: &JsonValue, what: &str) -> Result<RemoteConfigParameter, FirebaseError> {
    let object = as_object(value, what)?;
    let default_value = match object.get("defaultValue") {
        Some(inner) => parameter_value(inner, &format!("{what}.defaultValue"))?,
        None => None,
    };
    let mut conditional_values = BTreeMap::new();
    if let Some(values) = object.get("conditionalValues") {
        let values = as_object(values, &format!("{what}.conditionalValues"))?;
        for (name, inner) in values {
            if let Some(resolved) =
                parameter_value(inner, &format!("{what}.conditionalValues.{name}"))?
            {
                conditional_values.insert(name.clone(), resolved);
            }
        }
    }
    Ok(RemoteConfigParameter {
        default_value,
        conditional_values,
        description: optional_string(object, "description", what)?,
        value_type: optional_string(object, "valueType", what)?,
    })
}

fn parse_parameters(
    value: &JsonValue,
    what: &str,
) -> Result<BTreeMap<String, RemoteConfigParameter>, FirebaseError> {
    let object = as_object(value, what)?;
    let mut parameters = BTreeMap::new();
    for (key, inner) in object {
        parameters.insert(
            key.clone(),
            parse_parameter(inner, &format!("{what}.{key}"))?,
        );
    }
    Ok(parameters)
}

/// Parses and validates the fetched document. Lenient about unknown
/// fields (the REST surface is additive) and strict about the shapes it
/// resolves, so a corrupt template is named, not silently half-applied.
pub fn parse_template(document: &JsonValue) -> Result<RemoteConfigTemplate, FirebaseError> {
    let root = as_object(document, "the template")?;

    let mut conditions = Vec::new();
    if let Some(list) = root.get("conditions") {
        let list = list
            .as_array()
            .ok_or_else(|| invalid("conditions is not an array"))?;
        for (index, entry) in list.iter().enumerate() {
            let what = format!("conditions.{index}");
            let object = as_object(entry, &what)?;
            let condition = RemoteConfigCondition {
                name: required_string(object, "name", &what)?,
                expression: required_string(object, "expression", &what)?,
                tag_color: optional_string(object, "tagColor", &what)?,
            };
            if conditions
                .iter()
                .any(|parsed| parsed.name == condition.name)
            {
                return Err(invalid(format!(
                    "conditions.{index}.name `{}` is a duplicate",
                    condition.name
                )));
            }
            conditions.push(condition);
        }
    }

    let mut parameters = match root.get("parameters") {
        Some(value) => parse_parameters(value, "parameters")?,
        None => BTreeMap::new(),
    };

    if let Some(groups) = root.get("parameterGroups") {
        let groups = as_object(groups, "parameterGroups")?;
        for (group, inner) in groups {
            let what = format!("parameterGroups.{group}");
            let object = as_object(inner, &what)?;
            optional_string(object, "description", &what)?;
            let group_parameters = match object.get("parameters") {
                Some(value) => parse_parameters(value, &format!("{what}.parameters"))?,
                None => continue,
            };
            for key in group_parameters.keys() {
                if parameters.contains_key(key) {
                    return Err(invalid(format!(
                        "parameter `{key}` appears both at the top level and in parameter group `{group}`"
                    )));
                }
            }
            parameters.extend(group_parameters);
        }
    }

    let version = match root.get("version") {
        Some(value) if value.is_object() => Some(value.clone()),
        Some(_) => return Err(invalid("version is not an object")),
        None => None,
    };

    Ok(RemoteConfigTemplate {
        conditions,
        parameters,
        version,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_missing_optional_block_parses_empty() {
        let template = parse_template(&json!({})).unwrap();
        assert!(template.conditions.is_empty());
        assert!(template.parameters.is_empty());
        assert_eq!(template.resolved_values(), BTreeMap::new());
    }

    #[test]
    fn an_empty_parameter_value_object_stays_unset() {
        let template = parse_template(&json!({ "parameters": { "bare": {} } })).unwrap();
        assert!(template.parameters["bare"].default_value.is_none());
    }

    #[test]
    fn a_non_object_version_is_invalid() {
        let error = parse_template(&json!({ "version": "12" })).unwrap_err();
        assert_eq!(error.code, INVALID_TEMPLATE);
    }

    #[test]
    fn a_grouped_parameter_with_no_block_is_ignored() {
        let template = parse_template(&json!({
            "parameterGroups": { "checkout": { "description": "tuning" } }
        }))
        .unwrap();
        assert!(template.parameters.is_empty());
    }
}
