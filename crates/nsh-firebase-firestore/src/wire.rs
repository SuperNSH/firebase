//! The Firestore v1 REST wire format: the document URLs, the typed-value
//! encoding, the structured-query builder and the response/error mapping,
//! exactly as the REST reference
//! (<https://firebase.google.com/docs/firestore/reference/rest/v1>) writes
//! it and as the firebase-js-sdk's JsonProtoSerializer encodes values
//! (`@firebase/firestore` 4.18, `__PRIVATE_parseScalarValue`, `toNumber`,
//! `__PRIVATE_toInt32Proto`, `__PRIVATE_toQueryTarget`, the direction and
//! operator maps). `packages/nsh-firebase/src/firestore/wire.ts` mirrors
//! this name for name.

use serde_json::Value as JsonValue;
use serde_json::json;

use nsh_core::element::HttpResponse;
use nsh_firebase_core::error::{FirebaseError, INVALID_ARGUMENT};

/// The sandbox suite's Firestore emulator (emulator/firebase.json).
pub const EMULATOR_PORT: u16 = 18_080;
/// The production REST host; the emulator base wins in this sandbox.
pub const PRODUCTION_BASE: &str = "https://firestore.googleapis.com";
/// The database every call here names; named databases are not configured.
pub const DEFAULT_DATABASE: &str = "(default)";

/// The native handlers this crate routes through when a host is wired
/// (coordinator-fixed; see the crate docs for the full shapes).
pub mod native {
    pub const GET_DOC: &str = "firebase.firestore.getDoc";
    pub const SET_DOC: &str = "firebase.firestore.setDoc";
    pub const ADD_DOC: &str = "firebase.firestore.addDoc";
    pub const DELETE_DOC: &str = "firebase.firestore.deleteDoc";
    pub const RUN_QUERY: &str = "firebase.firestore.runQuery";
}

/// A getDoc reply that breaks its contract (`firestore/invalid-response`).
pub const INVALID_RESPONSE: &str = "firestore/invalid-response";
/// A value the JSON contract cannot carry (`firestore/invalid-data`):
/// non-finite doubles read back, and typed values beyond the seven this
/// client speaks (timestampValue, bytesValue, geoPointValue,
/// referenceValue and friends).
pub const INVALID_DATA: &str = "firestore/invalid-data";

/// The canonical error statuses the REST error body carries, each as the
/// `firestore/`-prefixed code the SDK's `FirestoreError` uses.
pub const CANCELLED: &str = "firestore/cancelled";
pub const UNKNOWN: &str = "firestore/unknown";
pub const INVALID_ARGUMENT_CODE: &str = "firestore/invalid-argument";
pub const DEADLINE_EXCEEDED: &str = "firestore/deadline-exceeded";
pub const NOT_FOUND: &str = "firestore/not-found";
pub const ALREADY_EXISTS: &str = "firestore/already-exists";
pub const PERMISSION_DENIED: &str = "firestore/permission-denied";
pub const UNAUTHENTICATED: &str = "firestore/unauthenticated";
pub const RESOURCE_EXHAUSTED: &str = "firestore/resource-exhausted";
pub const FAILED_PRECONDITION: &str = "firestore/failed-precondition";
pub const ABORTED: &str = "firestore/aborted";
pub const OUT_OF_RANGE: &str = "firestore/out-of-range";
pub const UNIMPLEMENTED: &str = "firestore/unimplemented";
pub const INTERNAL: &str = "firestore/internal";
pub const UNAVAILABLE: &str = "firestore/unavailable";
pub const DATA_LOSS: &str = "firestore/data-loss";

/// The largest integer the JSON contract carries exactly
/// (`Number.MAX_SAFE_INTEGER`); larger integers go over as doubles, as
/// the SDK's `isSafeInteger` gate does.
const MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;

pub fn invalid_argument(message: impl Into<String>) -> FirebaseError {
    FirebaseError::new(INVALID_ARGUMENT, message)
}

pub fn invalid_response(message: impl Into<String>) -> FirebaseError {
    FirebaseError::new(INVALID_RESPONSE, message)
}

pub fn invalid_data(message: impl Into<String>) -> FirebaseError {
    FirebaseError::new(INVALID_DATA, message)
}

/// The emulator base for `EMULATOR_PORT` (a host override keeps http).
pub fn emulator_base(host: Option<&str>) -> String {
    format!("http://{}:{EMULATOR_PORT}", host.unwrap_or("127.0.0.1"))
}

/// Percent-encodes one URL path segment, as `encodeURIComponent` does (the
/// SDK's `toUriEncodedString`); `(default)` keeps its parentheses.
pub fn encode_segment(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'!'
            | b'~'
            | b'*'
            | b'\''
            | b'('
            | b')' => out.push(byte as char),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// The documents root of the v1 surface:
/// `{base}/v1/projects/{project}/databases/(default)/documents`.
pub fn documents_base(rest_base: &str, project_id: &str) -> String {
    format!(
        "{}/v1/projects/{}/databases/{}/documents",
        rest_base.trim_end_matches('/'),
        encode_segment(project_id),
        encode_segment(DEFAULT_DATABASE),
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PathKind {
    Document,
    Collection,
}

/// Splits `path` into its segments, checking the shape Firestore gives
/// each kind: a document path names collection/document pairs (even
/// count), a collection path an odd count; ids may not be empty, `.`
/// or `..`.
fn split_path(path: &str, kind: PathKind) -> Result<Vec<String>, FirebaseError> {
    let segments: Vec<String> = path
        .trim_matches('/')
        .split('/')
        .map(str::to_owned)
        .collect();
    for segment in &segments {
        if segment.is_empty() {
            return Err(invalid_argument(format!(
                "the path `{path}` has an empty segment"
            )));
        }
        if segment == "." || segment == ".." {
            return Err(invalid_argument(format!(
                "the path `{path}` uses `.` or `..`; Firestore ids cannot"
            )));
        }
    }
    let even = segments.len().is_multiple_of(2);
    let fits = match kind {
        PathKind::Document => even,
        PathKind::Collection => !even,
    };
    if !fits {
        return Err(invalid_argument(match kind {
            PathKind::Document => format!(
                "not a document path: `{path}` must name collection/document pairs (an even segment count)"
            ),
            PathKind::Collection => format!(
                "not a collection path: `{path}` must name collections (an odd segment count)"
            ),
        }));
    }
    Ok(segments)
}

pub fn split_doc_path(path: &str) -> Result<Vec<String>, FirebaseError> {
    split_path(path, PathKind::Document)
}

pub fn split_collection_path(path: &str) -> Result<Vec<String>, FirebaseError> {
    split_path(path, PathKind::Collection)
}

fn encoded(segments: &[String]) -> String {
    let mut parts = Vec::with_capacity(segments.len());
    for segment in segments {
        parts.push(encode_segment(segment));
    }
    parts.join("/")
}

/// The URL of one document: `{base}/{segments}`, each segment
/// percent-encoded.
pub fn document_url(base: &str, path: &str) -> Result<String, FirebaseError> {
    let segments = split_doc_path(path)?;
    Ok(format!("{base}/{}", encoded(&segments)))
}

/// The URL one document is created under: the collection path itself,
/// whose last segment is the collectionId.
pub fn collection_url(base: &str, path: &str) -> Result<String, FirebaseError> {
    let segments = split_collection_path(path)?;
    Ok(format!("{base}/{}", encoded(&segments)))
}

/// What one runQuery needs on the wire: the POST URL (the parent path,
/// minus the collectionId, then `:runQuery`), the JSON body (`parent`
/// carried canonically, unencoded) and the native handler's args
/// (`{collectionPath, where?, orderBy?, limit?}`). One plan so both
/// tiers validate identically.
#[derive(Debug)]
pub struct QueryPlan {
    pub url: String,
    pub body: JsonValue,
    pub native_args: JsonValue,
}

/// The where filter the facade carries: an op is the SDK's client string
/// (`"=="`, `"array-contains"`, ...).
#[derive(Debug, Clone, PartialEq)]
pub struct WhereFilter {
    pub field: String,
    pub op: WhereOp,
    pub value: JsonValue,
}

/// The filter operators the wire speaks, in client and canonical form
/// (the SDK's `kt` map).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WhereOp {
    Equal,
    LessThan,
    LessThanOrEqual,
    GreaterThan,
    GreaterThanOrEqual,
    NotEqual,
    ArrayContains,
    In,
    NotIn,
    ArrayContainsAny,
}

impl WhereOp {
    /// The client string the facade's args carry.
    pub fn as_client(self) -> &'static str {
        match self {
            Self::Equal => "==",
            Self::LessThan => "<",
            Self::LessThanOrEqual => "<=",
            Self::GreaterThan => ">",
            Self::GreaterThanOrEqual => ">=",
            Self::NotEqual => "!=",
            Self::ArrayContains => "array-contains",
            Self::In => "in",
            Self::NotIn => "not-in",
            Self::ArrayContainsAny => "array-contains-any",
        }
    }

    /// The canonical name the structured query carries.
    pub fn as_wire(self) -> &'static str {
        match self {
            Self::Equal => "EQUAL",
            Self::LessThan => "LESS_THAN",
            Self::LessThanOrEqual => "LESS_THAN_OR_EQUAL",
            Self::GreaterThan => "GREATER_THAN",
            Self::GreaterThanOrEqual => "GREATER_THAN_OR_EQUAL",
            Self::NotEqual => "NOT_EQUAL",
            Self::ArrayContains => "ARRAY_CONTAINS",
            Self::In => "IN",
            Self::NotIn => "NOT_IN",
            Self::ArrayContainsAny => "ARRAY_CONTAINS_ANY",
        }
    }
}

/// One sort: a field and its direction.
#[derive(Debug, Clone, PartialEq)]
pub struct OrderBy {
    pub field: String,
    pub direction: Direction,
}

/// The sort directions, in client and canonical form (the SDK's `Ut` map).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Direction {
    #[default]
    Asc,
    Desc,
}

impl Direction {
    pub fn as_client(self) -> &'static str {
        match self {
            Self::Asc => "asc",
            Self::Desc => "desc",
        }
    }

    pub fn as_wire(self) -> &'static str {
        match self {
            Self::Asc => "ASCENDING",
            Self::Desc => "DESCENDING",
        }
    }
}

/// The structured query one runQuery runs: a shallow collection query
/// with one optional where filter, sorts, and a limit. Composite
/// filters, cursors and offsets have no facade surface.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FirestoreQuery {
    pub collection_path: String,
    pub r#where: Option<WhereFilter>,
    pub order_by: Vec<OrderBy>,
    pub limit: Option<u32>,
}

/// Builds the plan for `query` against `rest_base`. Every validation the
/// two tiers share lives here, so a refusal is identical whichever tier
/// answers.
pub fn run_query_plan(
    rest_base: &str,
    project_id: &str,
    query: &FirestoreQuery,
) -> Result<QueryPlan, FirebaseError> {
    let segments = split_collection_path(&query.collection_path)?;
    let (last, parent) = segments.split_last().expect("an odd path has a last");
    let mut body = serde_json::Map::new();
    body.insert(
        "parent".to_owned(),
        json!(canonical_parent(project_id, parent)),
    );
    let mut structured = serde_json::Map::new();
    structured.insert("from".to_owned(), json!([{ "collectionId": last }]));
    // Key order is load-bearing: the TS side builds the same literal, so
    // the JSON bodies match character for character.
    if let Some(filter) = &query.r#where {
        structured.insert("where".to_owned(), field_filter(filter)?);
    }
    if !query.order_by.is_empty() {
        let orders: Vec<JsonValue> = query
            .order_by
            .iter()
            .map(order_entry)
            .collect::<Result<_, _>>()?;
        structured.insert("orderBy".to_owned(), JsonValue::Array(orders));
    }
    if let Some(limit) = query.limit {
        if limit == 0 {
            return Err(invalid_argument("limit must be at least 1"));
        }
        structured.insert("limit".to_owned(), json!(limit));
    }
    body.insert("structuredQuery".to_owned(), JsonValue::Object(structured));
    let base = documents_base(rest_base, project_id);
    let url = if parent.is_empty() {
        format!("{base}:runQuery")
    } else {
        format!("{base}/{}:runQuery", encoded(parent))
    };
    Ok(QueryPlan {
        url,
        body: JsonValue::Object(body),
        native_args: native_query_args(query),
    })
}

/// The resource name of the query's parent, canonical (unencoded) as the
/// request body's `parent` carries it.
fn canonical_parent(project_id: &str, parent: &[String]) -> String {
    let base = format!("projects/{project_id}/databases/{DEFAULT_DATABASE}/documents");
    if parent.is_empty() {
        base
    } else {
        format!("{base}/{}", parent.join("/"))
    }
}

fn field_filter(filter: &WhereFilter) -> Result<JsonValue, FirebaseError> {
    if filter.field.is_empty() {
        return Err(invalid_argument("the where filter needs a field path"));
    }
    if matches!(
        filter.op,
        WhereOp::In | WhereOp::NotIn | WhereOp::ArrayContainsAny
    ) && !filter.value.is_array()
    {
        return Err(invalid_argument(format!(
            "`{}` needs an array value",
            filter.op.as_client()
        )));
    }
    Ok(json!({
        "fieldFilter": {
            "field": { "fieldPath": filter.field },
            "op": filter.op.as_wire(),
            "value": encode_value(&filter.value)?,
        }
    }))
}

fn order_entry(order: &OrderBy) -> Result<JsonValue, FirebaseError> {
    if order.field.is_empty() {
        return Err(invalid_argument("the orderBy needs a field path"));
    }
    Ok(json!({
        "field": { "fieldPath": order.field },
        "direction": order.direction.as_wire(),
    }))
}

/// The native handler's args: the contract's optional keys appear only
/// when set, in the contract's order.
fn native_query_args(query: &FirestoreQuery) -> JsonValue {
    let mut args = serde_json::Map::new();
    args.insert("collectionPath".to_owned(), json!(query.collection_path));
    if let Some(filter) = &query.r#where {
        args.insert(
            "where".to_owned(),
            json!({
                "field": filter.field,
                "op": filter.op.as_client(),
                "value": filter.value,
            }),
        );
    }
    if !query.order_by.is_empty() {
        let orders: Vec<JsonValue> = query
            .order_by
            .iter()
            .map(|order| {
                json!({
                    "field": order.field,
                    "direction": order.direction.as_client(),
                })
            })
            .collect();
        args.insert("orderBy".to_owned(), JsonValue::Array(orders));
    }
    if let Some(limit) = query.limit {
        args.insert("limit".to_owned(), json!(limit));
    }
    JsonValue::Object(args)
}

/// Encodes one JSON value as the REST typed value: null to the
/// `NULL_VALUE` sentinel, safe integers to their string form, `-0` to
/// the SDK's "-0" spelling.
pub fn encode_value(value: &JsonValue) -> Result<JsonValue, FirebaseError> {
    Ok(match value {
        JsonValue::Null => json!({ "nullValue": "NULL_VALUE" }),
        JsonValue::Bool(inner) => json!({ "booleanValue": inner }),
        JsonValue::Number(number) => encode_number(number)?,
        JsonValue::String(inner) => json!({ "stringValue": inner }),
        JsonValue::Array(items) => {
            let mut values = Vec::with_capacity(items.len());
            for item in items {
                values.push(encode_value(item)?);
            }
            json!({ "arrayValue": { "values": values } })
        }
        JsonValue::Object(entries) => {
            let mut fields = serde_json::Map::new();
            for (key, inner) in entries {
                fields.insert(key.clone(), encode_value(inner)?);
            }
            json!({ "mapValue": { "fields": fields } })
        }
    })
}

fn encode_number(number: &serde_json::Number) -> Result<JsonValue, FirebaseError> {
    let value = number
        .as_f64()
        .ok_or_else(|| invalid_data(format!("the number {number} has no wire form")))?;
    // -0 as a JSON number would lose its sign; the SDK spells it "-0".
    if value == 0.0 && value.is_sign_negative() {
        return Ok(json!({ "doubleValue": "-0" }));
    }
    if value.fract() == 0.0 && value.abs() <= MAX_SAFE_INTEGER as f64 {
        return Ok(json!({ "integerValue": format!("{}", value as i64) }));
    }
    let number = serde_json::Number::from_f64(value)
        .ok_or_else(|| invalid_data(format!("the number {value} has no wire form")))?;
    Ok(json!({ "doubleValue": number }))
}

/// Decodes one typed value back to JSON. Types beyond the seven this
/// client speaks (a timestamp written by another client, say) are an
/// [`INVALID_DATA`] error, not a silent loss.
pub fn decode_value(typed: &JsonValue) -> Result<JsonValue, FirebaseError> {
    let Some(entries) = typed.as_object() else {
        return Err(invalid_data("the value is not a typed value object"));
    };
    if entries.contains_key("nullValue") {
        return Ok(JsonValue::Null);
    }
    if let Some(inner) = entries.get("booleanValue") {
        return inner
            .as_bool()
            .map(JsonValue::Bool)
            .ok_or_else(|| invalid_data("the booleanValue is not a boolean"));
    }
    if let Some(inner) = entries.get("integerValue") {
        return decode_integer(inner);
    }
    if let Some(inner) = entries.get("doubleValue") {
        return decode_double(inner);
    }
    if let Some(inner) = entries.get("stringValue") {
        return inner
            .as_str()
            .map(|text| JsonValue::String(text.to_owned()))
            .ok_or_else(|| invalid_data("the stringValue is not a string"));
    }
    if let Some(inner) = entries.get("arrayValue") {
        let items = inner
            .get("values")
            .and_then(JsonValue::as_array)
            .cloned()
            .unwrap_or_default();
        let mut decoded = Vec::with_capacity(items.len());
        for item in &items {
            decoded.push(decode_value(item)?);
        }
        return Ok(JsonValue::Array(decoded));
    }
    if let Some(inner) = entries.get("mapValue") {
        return decode_fields(inner.get("fields"));
    }
    let named = entries.keys().next().map(String::as_str).unwrap_or("?");
    Err(invalid_data(format!(
        "the value type `{named}` has no JSON form; this client carries string, integer, double, boolean, null, map and array values only"
    )))
}

fn decode_integer(inner: &JsonValue) -> Result<JsonValue, FirebaseError> {
    let Some(text) = inner.as_str() else {
        return Err(invalid_data(format!(
            "the integerValue {inner} is not a string"
        )));
    };
    let Ok(parsed) = text.parse::<i64>() else {
        return Err(invalid_data(format!(
            "the integerValue `{text}` is not a number"
        )));
    };
    // Beyond the safe range, TypeScript's Number() already lost
    // precision; land in the same float here.
    if parsed.abs() <= MAX_SAFE_INTEGER {
        return Ok(json!(parsed));
    }
    let number = serde_json::Number::from_f64(parsed as f64)
        .ok_or_else(|| invalid_data(format!("the integerValue `{text}` is not a number")))?;
    Ok(JsonValue::Number(number))
}

fn decode_double(inner: &JsonValue) -> Result<JsonValue, FirebaseError> {
    let number = match inner {
        JsonValue::Number(number) => number.clone(),
        // Only the SDK's spellings decode; other strings are not on the
        // wire.
        JsonValue::String(text) => decode_double_text(text)?,
        _ => {
            return Err(invalid_data(format!(
                "the doubleValue {inner} is not a number"
            )));
        }
    };
    Ok(JsonValue::Number(number))
}

fn decode_double_text(text: &str) -> Result<serde_json::Number, FirebaseError> {
    if matches!(text, "NaN" | "Infinity" | "-Infinity") {
        return Err(invalid_data(format!(
            "the doubleValue `{text}` has no JSON form; JSON cannot carry it"
        )));
    }
    let parsed = text
        .parse::<f64>()
        .map_err(|_| invalid_data(format!("the doubleValue `{text}` is not a number")))?;
    serde_json::Number::from_f64(parsed)
        .ok_or_else(|| invalid_data(format!("the doubleValue `{text}` is not a number")))
}

/// Encodes the document data (a JSON object of fields) as the wire's
/// `fields` map.
pub fn encode_fields(data: &JsonValue) -> Result<JsonValue, FirebaseError> {
    let entries = data
        .as_object()
        .ok_or_else(|| invalid_argument("the document data must be a JSON object of fields"))?;
    let mut fields = serde_json::Map::new();
    for (key, value) in entries {
        fields.insert(key.clone(), encode_value(value)?);
    }
    Ok(JsonValue::Object(fields))
}

/// Decodes the wire's `fields` map (absent: an empty object).
pub fn decode_fields(fields: Option<&JsonValue>) -> Result<JsonValue, FirebaseError> {
    let Some(entries) = fields.map(JsonValue::as_object) else {
        return Ok(JsonValue::Object(serde_json::Map::new()));
    };
    let Some(entries) = entries else {
        return Err(invalid_response("the document's `fields` is not an object"));
    };
    let mut out = serde_json::Map::new();
    for (key, typed) in entries {
        out.insert(key.clone(), decode_value(typed)?);
    }
    Ok(JsonValue::Object(out))
}

/// The leaf field paths of `data` (dot-joined; maps descend, arrays and
/// empty maps are leaves): what a merge write sends as
/// `updateMask.fieldPaths`. Keys a dot path cannot address (`.`,
/// backtick, `~`, `*`, `/`, `\`) are refused — the server would read them
/// as other fields.
pub fn leaf_paths(data: &JsonValue) -> Result<Vec<String>, FirebaseError> {
    let entries = data
        .as_object()
        .ok_or_else(|| invalid_argument("the document data must be a JSON object of fields"))?;
    if entries.is_empty() {
        return Err(invalid_argument(
            "a merge write needs at least one field; the REST patch mask cannot be empty",
        ));
    }
    let mut paths = Vec::new();
    collect_leaf_paths(data, String::new(), &mut paths)?;
    Ok(paths)
}

fn collect_leaf_paths(
    value: &JsonValue,
    prefix: String,
    paths: &mut Vec<String>,
) -> Result<(), FirebaseError> {
    match value {
        JsonValue::Object(entries) if !entries.is_empty() => {
            for (key, inner) in entries {
                if key.contains('.')
                    || key.contains('`')
                    || key.contains('~')
                    || key.contains('*')
                    || key.contains('/')
                    || key.contains('\\')
                {
                    return Err(invalid_argument(format!(
                        "the field key `{key}` cannot be addressed by a merge mask; write it without merge"
                    )));
                }
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                collect_leaf_paths(inner, path, paths)?;
            }
            Ok(())
        }
        // An empty map is itself the leaf value.
        _ => {
            paths.push(prefix);
            Ok(())
        }
    }
}

/// The merge patch's query string: one `updateMask.fieldPaths=` pair per
/// leaf path, in the data's order.
pub fn merge_mask_query(paths: &[String]) -> String {
    paths
        .iter()
        .map(|path| format!("updateMask.fieldPaths={}", encode_segment(path)))
        .collect::<Vec<_>>()
        .join("&")
}

/// The document id: the last segment of the resource name.
pub fn id_from_name(name: &str) -> Result<String, FirebaseError> {
    name.rsplit('/')
        .next()
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| invalid_response(format!("the document name `{name}` has no id")))
}

/// One query result: the id the server assigned and its JSON data.
#[derive(Debug, Clone, PartialEq)]
pub struct QueryDoc {
    pub id: String,
    pub data: JsonValue,
}

/// A getDoc outcome: `data` is none exactly when the document is absent
/// (the REST read answers 404, which the facade turns into
/// `exists: false`).
#[derive(Debug, Clone, PartialEq)]
pub struct DocSnapshot {
    pub exists: bool,
    pub data: Option<JsonValue>,
}

/// Maps a query result document to its id and JSON data.
pub fn query_doc_from(document: &JsonValue) -> Result<QueryDoc, FirebaseError> {
    let name = document
        .get("name")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| invalid_response("the document answered without a name"))?;
    Ok(QueryDoc {
        id: id_from_name(name)?,
        data: decode_fields(document.get("fields"))?,
    })
}

/// Maps a present document (a getDoc or createDocument reply) to a
/// snapshot.
pub fn snapshot_from(document: &JsonValue) -> Result<DocSnapshot, FirebaseError> {
    Ok(DocSnapshot {
        exists: true,
        data: Some(decode_fields(document.get("fields"))?),
    })
}

/// Parses a JSON reply body (a non-JSON 2xx body is a broken server, not
/// a network loss).
pub fn parse_body(text: &str) -> Result<JsonValue, FirebaseError> {
    serde_json::from_str(text)
        .map_err(|_| invalid_response("the server sent a body that is not JSON"))
}

/// Maps a runQuery reply (the REST streaming shape: a JSON array whose
/// items carry `document` and `readTime`, the trailing ones only
/// `readTime`/`done`) to its documents.
pub fn docs_from_run_query(text: &str) -> Result<Vec<QueryDoc>, FirebaseError> {
    let items = parse_body(text)?
        .as_array()
        .ok_or_else(|| invalid_response("the runQuery response is not an array"))?
        .clone();
    let mut docs = Vec::new();
    for item in &items {
        // The stream's tail items mark progress, not documents.
        let Some(document) = item.get("document") else {
            continue;
        };
        docs.push(query_doc_from(document)?);
    }
    Ok(docs)
}

/// A 2xx stands; anything else maps through the REST error body.
pub fn ensure_ok(response: HttpResponse) -> Result<HttpResponse, FirebaseError> {
    if (200..300).contains(&response.status()) {
        return Ok(response);
    }
    Err(error_for_response(response.status(), &response.text()))
}

/// The canonical statuses the REST error body carries
/// (`{error: {code, message, status}}`), to their client codes.
fn canonical_client_code(canonical: &str) -> Option<&'static str> {
    Some(match canonical {
        "CANCELLED" => CANCELLED,
        "UNKNOWN" => UNKNOWN,
        "INVALID_ARGUMENT" => INVALID_ARGUMENT_CODE,
        "DEADLINE_EXCEEDED" => DEADLINE_EXCEEDED,
        "NOT_FOUND" => NOT_FOUND,
        "ALREADY_EXISTS" => ALREADY_EXISTS,
        "PERMISSION_DENIED" => PERMISSION_DENIED,
        "UNAUTHENTICATED" => UNAUTHENTICATED,
        "RESOURCE_EXHAUSTED" => RESOURCE_EXHAUSTED,
        "FAILED_PRECONDITION" => FAILED_PRECONDITION,
        "ABORTED" => ABORTED,
        "OUT_OF_RANGE" => OUT_OF_RANGE,
        "UNIMPLEMENTED" => UNIMPLEMENTED,
        "INTERNAL" => INTERNAL,
        "UNAVAILABLE" => UNAVAILABLE,
        "DATA_LOSS" => DATA_LOSS,
        _ => return None,
    })
}

/// The HTTP status fallback (google/rpc/code.proto's mapping, as the
/// SDK's `mapCodeFromRpcCode` applies it), used when the body names no
/// status.
fn client_code_for_status(status: u16) -> &'static str {
    match status {
        400 => INVALID_ARGUMENT_CODE,
        401 => UNAUTHENTICATED,
        403 => PERMISSION_DENIED,
        404 => NOT_FOUND,
        409 => ABORTED,
        429 => RESOURCE_EXHAUSTED,
        499 => CANCELLED,
        500 => INTERNAL,
        501 => UNIMPLEMENTED,
        503 => UNAVAILABLE,
        504 => DEADLINE_EXCEEDED,
        _ => UNKNOWN,
    }
}

/// The error a non-2xx reply carries: the body's canonical `status`
/// names the code (an unknown name is `firestore/unknown`), its
/// `message` is the message, and the HTTP status stands when the body
/// carries neither.
pub fn error_for_response(status: u16, text: &str) -> FirebaseError {
    let body: JsonValue = serde_json::from_str(text).unwrap_or(JsonValue::Null);
    let error = body.get("error");
    // A body status always names the code: recognized canonical names
    // map, an unrecognized one is UNKNOWN; only an absent status falls
    // back to the HTTP status, like the js-sdk's rpc-status mapping.
    let code = match error
        .and_then(|error| error.get("status"))
        .and_then(JsonValue::as_str)
    {
        Some(canonical) => canonical_client_code(canonical).unwrap_or(UNKNOWN),
        None => client_code_for_status(status),
    };
    let message = error
        .and_then(|error| error.get("message"))
        .and_then(JsonValue::as_str)
        .unwrap_or(text);
    FirebaseError::new(code, format!("{message} ({status})"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_encode_to_the_wire_shapes() {
        assert_eq!(
            encode_value(&json!("one")).unwrap(),
            json!({ "stringValue": "one" })
        );
        assert_eq!(
            encode_value(&json!(true)).unwrap(),
            json!({ "booleanValue": true })
        );
        assert_eq!(
            encode_value(&json!(null)).unwrap(),
            json!({ "nullValue": "NULL_VALUE" })
        );
        assert_eq!(
            encode_value(&json!(3)).unwrap(),
            json!({ "integerValue": "3" })
        );
        assert_eq!(
            encode_value(&json!(1.5)).unwrap(),
            json!({ "doubleValue": 1.5 })
        );
        // At the safe-integer boundary and past it: integer, then double.
        assert_eq!(
            encode_value(&json!(9_007_199_254_740_991i64)).unwrap(),
            json!({ "integerValue": "9007199254740991" })
        );
        assert_eq!(
            encode_value(&json!(9_007_199_254_740_993i64)).unwrap(),
            json!({ "doubleValue": 9_007_199_254_740_992.0 })
        );
        assert_eq!(
            encode_value(&json!(-0.0)).unwrap(),
            json!({ "doubleValue": "-0" })
        );
        // NaN and the infinities cannot exist in a JsonValue at all (the
        // TypeScript side refuses them as invalid-data, since JSON has no
        // form for them); the wire's spellings only ever appear on decode,
        // where they fail.
        let not_a_number = json!({ "doubleValue": "NaN" });
        let error = decode_value(&not_a_number).unwrap_err();
        assert_eq!(error.code, INVALID_DATA);
    }

    #[test]
    fn containers_encode_recursively_and_keep_key_order() {
        let data = json!({
            "tags": ["a", 2, null],
            "meta": { "deep": 1.5, "ok": true, "note": null, "empty": {} },
            "none": [],
        });
        let encoded = encode_value(&data).unwrap();
        assert_eq!(
            encoded["mapValue"]["fields"]["tags"],
            json!({ "arrayValue": { "values": [
                { "stringValue": "a" }, { "integerValue": "2" }, { "nullValue": "NULL_VALUE" },
            ] } })
        );
        let empty = encoded["mapValue"]["fields"]["meta"]["mapValue"]["fields"]["empty"].clone();
        assert_eq!(empty, json!({ "mapValue": { "fields": {} } }));
        // An empty array keeps its (empty) values list.
        assert_eq!(
            encoded["mapValue"]["fields"]["none"],
            json!({ "arrayValue": { "values": [] } })
        );
        // The object's key order survives: the TS side builds the same
        // literal order, so the JSON bodies match character for character.
        let text = serde_json::to_string(&encoded).unwrap();
        let at = |marker: &str| text.find(marker).expect(marker);
        assert!(at(r#""tags""#) < at(r#""meta""#));
        assert!(at(r#""meta""#) < at(r#""none""#));
        assert!(at(r#""deep""#) < at(r#""ok""#));
        assert!(at(r#""ok""#) < at(r#""note""#));
        assert!(at(r#""note""#) < at(r#""empty""#));
    }

    #[test]
    fn values_decode_from_the_wire_shapes() {
        assert_eq!(
            decode_value(&json!({ "stringValue": "one" })).unwrap(),
            json!("one")
        );
        assert_eq!(
            decode_value(&json!({ "booleanValue": false })).unwrap(),
            json!(false)
        );
        assert_eq!(
            decode_value(&json!({ "integerValue": "3" })).unwrap(),
            json!(3)
        );
        assert_eq!(
            decode_value(&json!({ "doubleValue": 1.5 })).unwrap(),
            json!(1.5)
        );
        // Past the safe range the float stands, as TypeScript's Number().
        let big = json!({ "integerValue": "9007199254740993" });
        assert_eq!(decode_value(&big).unwrap(), json!(9_007_199_254_740_992.0));
        assert_eq!(
            decode_value(&json!({ "nullValue": "NULL_VALUE" })).unwrap(),
            JsonValue::Null
        );
        let nested = json!({ "mapValue": { "fields": { "a": { "integerValue": "1" } } } });
        assert_eq!(decode_value(&nested).unwrap(), json!({ "a": 1 }));
        // Absent containers decode empty, as the server omits them.
        assert_eq!(decode_value(&json!({ "mapValue": {} })).unwrap(), json!({}));
        assert_eq!(
            decode_value(&json!({ "arrayValue": {} })).unwrap(),
            json!([])
        );
    }

    #[test]
    fn values_the_json_contract_cannot_carry_are_invalid_data() {
        let timestamp = json!({ "timestampValue": "2026-01-01T00:00:00Z" });
        let error = decode_value(&timestamp).unwrap_err();
        assert_eq!(error.code, INVALID_DATA);
        assert!(error.message.contains("timestampValue"), "{error}");
        let nan = json!({ "doubleValue": "NaN" });
        assert_eq!(decode_value(&nan).unwrap_err().code, INVALID_DATA);
        let gibberish = json!({ "doubleValue": "soon" });
        assert_eq!(decode_value(&gibberish).unwrap_err().code, INVALID_DATA);
        let scalar = json!(3);
        assert_eq!(decode_value(&scalar).unwrap_err().code, INVALID_DATA);
    }

    #[test]
    fn a_value_round_trips_through_the_wire() {
        let data = json!({
            "done": false, "title": "one", "count": 3, "price": 1.5,
            "tags": ["a", 2], "meta": { "deep": { "note": null } },
        });
        let decoded = decode_value(&encode_value(&data).unwrap()).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn document_paths_validate_shape_and_encode() {
        let base = documents_base("http://127.0.0.1:18080/", "demo-nsh");
        assert_eq!(
            base,
            "http://127.0.0.1:18080/v1/projects/demo-nsh/databases/(default)/documents"
        );
        // A trailing slash is trimmed, `(default)` keeps its parentheses,
        // and ids with spaces or question marks percent-encode per segment.
        let spaced = document_url(&base, "cities/a b?c").unwrap();
        assert_eq!(spaced, format!("{base}/cities/a%20b%3Fc"));
        let nested = collection_url(&base, "groups/g1/people").unwrap();
        assert_eq!(nested, format!("{base}/groups/g1/people"));
        let odd = document_url(&base, "cities").unwrap_err();
        assert_eq!(odd.code, INVALID_ARGUMENT);
        assert!(odd.message.contains("even segment count"), "{odd}");
        let even = collection_url(&base, "cities/LA").unwrap_err();
        assert_eq!(even.code, INVALID_ARGUMENT);
        assert!(even.message.contains("odd segment count"), "{even}");
        let dots = document_url(&base, "cities/../LA").unwrap_err();
        assert_eq!(dots.code, INVALID_ARGUMENT);
        let empty = document_url(&base, "cities//LA").unwrap_err();
        assert_eq!(empty.code, INVALID_ARGUMENT);
        assert!(empty.message.contains("empty segment"), "{empty}");
    }

    #[test]
    fn the_query_plan_carries_url_body_and_native_args() {
        let where_state = WhereFilter {
            field: "state".to_owned(),
            op: WhereOp::Equal,
            value: json!("CA"),
        };
        let by_pop = OrderBy {
            field: "pop".to_owned(),
            direction: Direction::Desc,
        };
        let query = FirestoreQuery {
            collection_path: "ui-groups/g1/cities".to_owned(),
            r#where: Some(where_state),
            order_by: vec![by_pop],
            limit: Some(2),
        };
        let plan = run_query_plan("http://127.0.0.1:18080", "demo-nsh", &query).unwrap();
        // The URL parent is the collection's parent path; the body's
        // `parent` is the canonical, unencoded resource name.
        assert_eq!(
            plan.url,
            "http://127.0.0.1:18080/v1/projects/demo-nsh/databases/(default)/documents/ui-groups/g1:runQuery"
        );
        assert_eq!(
            plan.body,
            json!({
                "parent": "projects/demo-nsh/databases/(default)/documents/ui-groups/g1",
                "structuredQuery": {
                    "from": [{ "collectionId": "cities" }],
                    "where": {
                        "fieldFilter": {
                            "field": { "fieldPath": "state" },
                            "op": "EQUAL",
                            "value": { "stringValue": "CA" },
                        }
                    },
                    "orderBy": [{ "field": { "fieldPath": "pop" }, "direction": "DESCENDING" }],
                    "limit": 2,
                }
            })
        );
        assert_eq!(
            plan.native_args,
            json!({
                "collectionPath": "ui-groups/g1/cities",
                "where": { "field": "state", "op": "==", "value": "CA" },
                "orderBy": [{ "field": "pop", "direction": "desc" }],
                "limit": 2,
            })
        );
    }

    #[test]
    fn a_top_level_query_posts_at_the_documents_root_and_omits_empty_keys() {
        let query = FirestoreQuery {
            collection_path: "cities".to_owned(),
            limit: Some(5),
            ..FirestoreQuery::default()
        };
        let plan = run_query_plan("http://127.0.0.1:18080", "demo-nsh", &query).unwrap();
        assert_eq!(
            plan.url,
            "http://127.0.0.1:18080/v1/projects/demo-nsh/databases/(default)/documents:runQuery"
        );
        assert_eq!(
            plan.body,
            json!({
                "parent": "projects/demo-nsh/databases/(default)/documents",
                "structuredQuery": {
                    "from": [{ "collectionId": "cities" }],
                    "limit": 5,
                }
            })
        );
        // The native args carry only the keys the contract names, only
        // when set.
        assert_eq!(
            plan.native_args,
            json!({ "collectionPath": "cities", "limit": 5 })
        );
    }

    #[test]
    fn query_validation_refuses_empty_fields_bad_limits_and_scalar_in() {
        let base = "http://127.0.0.1:18080";
        let empty_field = FirestoreQuery {
            collection_path: "cities".to_owned(),
            r#where: Some(WhereFilter {
                field: String::new(),
                op: WhereOp::Equal,
                value: json!(1),
            }),
            ..FirestoreQuery::default()
        };
        let error = run_query_plan(base, "p", &empty_field).unwrap_err();
        assert_eq!(error.code, INVALID_ARGUMENT);
        let zero_limit = FirestoreQuery {
            collection_path: "cities".to_owned(),
            limit: Some(0),
            ..FirestoreQuery::default()
        };
        let error = run_query_plan(base, "p", &zero_limit).unwrap_err();
        assert_eq!(error.code, INVALID_ARGUMENT);
        assert_eq!(error.message, "limit must be at least 1");
        let scalar_in = FirestoreQuery {
            collection_path: "cities".to_owned(),
            r#where: Some(WhereFilter {
                field: "state".to_owned(),
                op: WhereOp::In,
                value: json!("CA"),
            }),
            ..FirestoreQuery::default()
        };
        let error = run_query_plan(base, "p", &scalar_in).unwrap_err();
        assert_eq!(error.message, "`in` needs an array value");
        let odd_path = FirestoreQuery {
            collection_path: "cities/LA".to_owned(),
            ..FirestoreQuery::default()
        };
        let error = run_query_plan(base, "p", &odd_path).unwrap_err();
        assert_eq!(error.code, INVALID_ARGUMENT);
    }

    #[test]
    fn merge_leaf_paths_descend_maps_and_stop_at_arrays() {
        let data = json!({
            "title": "Merged",
            "meta": { "deep": 2, "list": [1, 2], "void": {} },
        });
        let paths = leaf_paths(&data).unwrap();
        let want = vec![
            "title".to_owned(),
            "meta.deep".to_owned(),
            "meta.list".to_owned(),
            "meta.void".to_owned(),
        ];
        assert_eq!(paths, want);
        let query = merge_mask_query(&paths);
        assert_eq!(
            query,
            "updateMask.fieldPaths=title&updateMask.fieldPaths=meta.deep\
             &updateMask.fieldPaths=meta.list&updateMask.fieldPaths=meta.void"
        );
        // An empty merge has no REST mask shape: refused, not a silent
        // replace.
        let error = leaf_paths(&json!({})).unwrap_err();
        assert_eq!(
            error.message,
            "a merge write needs at least one field; the REST patch mask cannot be empty"
        );
        let dotted = leaf_paths(&json!({ "a.b": 1 })).unwrap_err();
        assert_eq!(dotted.code, INVALID_ARGUMENT);
        assert!(
            dotted
                .message
                .contains("cannot be addressed by a merge mask")
        );
    }

    #[test]
    fn fields_encode_and_decode_in_order() {
        let data = json!({ "done": false, "title": "one", "count": 3 });
        let fields = encode_fields(&data).unwrap();
        assert_eq!(
            serde_json::to_string(&fields).unwrap(),
            r#"{"done":{"booleanValue":false},"title":{"stringValue":"one"},"count":{"integerValue":"3"}}"#
        );
        assert_eq!(decode_fields(Some(&fields)).unwrap(), data);
        // Absent fields decode to an empty document; broken ones are a
        // server shape error.
        assert_eq!(decode_fields(None).unwrap(), json!({}));
        let broken = json!("x");
        assert_eq!(
            decode_fields(Some(&broken)).unwrap_err().code,
            INVALID_RESPONSE
        );
        assert_eq!(
            encode_fields(&json!([1])).unwrap_err().code,
            INVALID_ARGUMENT
        );
    }

    #[test]
    fn ids_come_from_the_names_last_segment() {
        let full = "projects/p/databases/(default)/documents/cities/LA";
        assert_eq!(id_from_name(full).unwrap(), "LA");
        assert_eq!(id_from_name("cities/one").unwrap(), "one");
        // A name with no id is broken server output.
        assert_eq!(id_from_name("").unwrap_err().code, INVALID_RESPONSE);
        let trailing = "projects/p/databases/(default)/documents/cities/";
        assert_eq!(id_from_name(trailing).unwrap_err().code, INVALID_RESPONSE);
    }

    #[test]
    fn documents_map_to_docs_and_snapshots() {
        let document = json!({
            "name": "projects/p/databases/(default)/documents/ui-cities/SERVER1",
            "fields": { "pop": { "integerValue": "500" } },
            "createTime": "2026-01-01T00:00:00Z",
            "updateTime": "2026-01-01T00:00:00Z",
        });
        let doc = query_doc_from(&document).unwrap();
        assert_eq!(doc.id, "SERVER1");
        assert_eq!(doc.data, json!({ "pop": 500 }));
        let snapshot = snapshot_from(&document).unwrap();
        assert!(snapshot.exists);
        assert_eq!(snapshot.data, Some(json!({ "pop": 500 })));
    }

    #[test]
    fn the_run_query_reply_yields_only_document_items() {
        let reply = r#"[
            {"document": {"name": "projects/p/databases/(default)/documents/ui-cities/SERVER1",
                          "fields": {"pop": {"integerValue": "500"}}},
             "readTime": "2026-01-01T00:00:00.000000001Z"},
            {"document": {"name": "projects/p/databases/(default)/documents/ui-cities/SERVER2",
                          "fields": {"pop": {"integerValue": "300"}}},
             "readTime": "2026-01-01T00:00:00.000000001Z"},
            {"readTime": "2026-01-01T00:00:00.000000001Z"},
            {"done": true, "readTime": "2026-01-01T00:00:00.000000001Z"}
        ]"#;
        let docs = docs_from_run_query(reply).unwrap();
        let ids: Vec<&str> = docs.iter().map(|doc| doc.id.as_str()).collect();
        assert_eq!(ids, vec!["SERVER1", "SERVER2"]);
        assert_eq!(docs[1].data, json!({ "pop": 300 }));
        // An empty result set and a broken shape are distinct.
        assert_eq!(docs_from_run_query("[]").unwrap(), Vec::new());
        let object = docs_from_run_query(r#"{"document": {}}"#).unwrap_err();
        assert_eq!(object.code, INVALID_RESPONSE);
        assert_eq!(
            docs_from_run_query("not-json").unwrap_err().code,
            INVALID_RESPONSE
        );
    }

    #[test]
    fn encode_segment_is_encode_uri_component() {
        assert_eq!(encode_segment("a b?c/d"), "a%20b%3Fc%2Fd");
        assert_eq!(encode_segment("(default)"), "(default)");
        assert_eq!(
            encode_segment("plain-id_1.txt!~*'()"),
            "plain-id_1.txt!~*'()"
        );
        assert_eq!(encode_segment("\u{e9}"), "%C3%A9");
    }

    #[test]
    fn server_errors_map_through_the_canonical_status() {
        let denied_body = r#"{"error": {"code": 403, "message": "Missing or insufficient permissions.", "status": "PERMISSION_DENIED"}}"#;
        let denied = error_for_response(403, denied_body);
        assert_eq!(denied.code, PERMISSION_DENIED);
        assert_eq!(denied.message, "Missing or insufficient permissions. (403)");
        let missing_body =
            r#"{"error": {"code": 404, "message": "no doc", "status": "NOT_FOUND"}}"#;
        assert_eq!(error_for_response(404, missing_body).code, NOT_FOUND);
        // The body's status stands over the HTTP status.
        let mismatched_body =
            r#"{"error": {"code": 400, "message": "bad", "status": "INVALID_ARGUMENT"}}"#;
        let mismatched = error_for_response(500, mismatched_body);
        assert_eq!(mismatched.code, INVALID_ARGUMENT_CODE);
        // An unknown canonical name is unknown, not the status default.
        let weird_body = r#"{"error": {"message": "m", "status": "WEIRD"}}"#;
        assert_eq!(error_for_response(400, weird_body).code, UNKNOWN);
        // Without a body, the HTTP status names the code.
        assert_eq!(error_for_response(404, "").code, NOT_FOUND);
        assert_eq!(error_for_response(400, "oops").code, INVALID_ARGUMENT_CODE);
        assert_eq!(error_for_response(401, "").code, UNAUTHENTICATED);
        assert_eq!(error_for_response(409, "").code, ABORTED);
        assert_eq!(error_for_response(429, "").code, RESOURCE_EXHAUSTED);
        assert_eq!(error_for_response(503, "").code, UNAVAILABLE);
        assert_eq!(error_for_response(418, "").code, UNKNOWN);
    }
}
