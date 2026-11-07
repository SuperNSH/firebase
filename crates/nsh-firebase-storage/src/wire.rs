//! The Cloud Storage REST wire format: percent-encoding, URLs, and the
//! object-metadata mapping (server JSON <-> fields), as the firebase-js-sdk
//! storage layer speaks it (`packages/storage/src/implementation/*.ts`).
//! `packages/nsh-firebase/src/storage/wire.ts` mirrors this name for name.

use std::collections::BTreeMap;

use serde_json::Value as JsonValue;

use nsh_firebase_core::FirebaseOptions;

/// A slice must be a multiple of this, except the last
/// (`RESUMABLE_UPLOAD_CHUNK_SIZE` in the SDK).
pub const CHUNK_SIZE: usize = 256 * 1024;

/// The storage error codes, `storage/`-prefixed like the SDK's.
pub const CANCELED: &str = "storage/canceled";
pub const UNKNOWN: &str = "storage/unknown";
pub const OBJECT_NOT_FOUND: &str = "storage/object-not-found";
pub const QUOTA_EXCEEDED: &str = "storage/quota-exceeded";
pub const UNAUTHENTICATED: &str = "storage/unauthenticated";
pub const UNAUTHORIZED: &str = "storage/unauthorized";
pub const RETRY_LIMIT_EXCEEDED: &str = "storage/retry-limit-exceeded";
pub const NO_DOWNLOAD_TOKEN: &str = "storage/no-download-token";
pub const NO_DEFAULT_BUCKET: &str = "storage/no-default-bucket";
pub const SERVER_FILE_WRONG_SIZE: &str = "storage/server-wrong-file-size";

/// Percent-encodes a URL component exactly as `encodeURIComponent` does,
/// so the server's decode and the JS SDK's agree byte for byte.
pub fn encode_component(text: &str) -> String {
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

/// The emulator switch answers a bare origin; the real config names the
/// full objects URL. Either way storage talks at `{origin}/v0/b/<bucket>/o`.
pub fn origin(options: &FirebaseOptions) -> Result<String, nsh_firebase_core::FirebaseError> {
    if let Some(endpoint) = &options.emulator.storage {
        return Ok(endpoint.base_url());
    }
    options
        .storage_bucket
        .as_ref()
        .map(|_| "https://firebasestorage.googleapis.com".to_owned())
        .ok_or_else(|| {
            nsh_firebase_core::FirebaseError::new(
                NO_DEFAULT_BUCKET,
                "FirebaseOptions.storageBucket is empty",
            )
        })
}

/// The objects URL of a bucket: uploads start, lists and downloads hang
/// off it (`location.bucketOnlyServerUrl()` / `fullServerUrl()`).
pub fn objects_base(origin: &str, bucket: &str) -> String {
    format!("{origin}/v0/b/{}/o", encode_component(bucket))
}

/// The object URL: the whole path encoded, slashes included, exactly as
/// the SDK's `fullServerUrl()` does.
pub fn object_url(objects_base: &str, path: &str) -> String {
    format!("{objects_base}/{}", encode_component(path))
}

/// The URL a resumable upload starts at.
pub fn start_url(objects_base: &str, path: &str) -> String {
    format!("{}?name={}", objects_base, encode_component(path))
}

/// The last path component (`_lastPartOfPath` in the SDK).
fn last_component(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_owned()
}

/// The object metadata a response describes; `full_path` is the server's
/// `name`, `name` its last component, like the SDK's mappings.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ObjectMetadata {
    pub bucket: Option<String>,
    pub generation: Option<String>,
    pub metageneration: Option<String>,
    pub full_path: String,
    pub name: String,
    pub size: Option<u64>,
    pub time_created: Option<String>,
    pub updated: Option<String>,
    pub md5_hash: Option<String>,
    pub cache_control: Option<String>,
    pub content_disposition: Option<String>,
    pub content_encoding: Option<String>,
    pub content_language: Option<String>,
    pub content_type: Option<String>,
    pub custom_metadata: Option<BTreeMap<String, String>>,
    pub download_tokens: Option<String>,
}

impl ObjectMetadata {
    /// The first download token's URL (`downloadUrlFromResourceString`):
    /// `{origin}/v0/b/<bucket>/o/<encoded path>?alt=media&token=<token>`.
    pub fn download_url(&self, origin: &str) -> Option<String> {
        let bucket = self.bucket.as_deref().unwrap_or_default();
        let token = self.download_tokens.as_deref()?.split(',').next()?;
        if token.is_empty() {
            return None;
        }
        Some(format!(
            "{origin}/v0/b/{}/o/{}?alt=media&token={}",
            encode_component(bucket),
            encode_component(&self.full_path),
            encode_component(token),
        ))
    }

    /// Parses a server metadata document (`fromResourceString` plus the
    /// `downloadTokens` list the download URL is built from).
    pub fn parse(json: &JsonValue) -> ObjectMetadata {
        let string = |key: &str| json.get(key).and_then(JsonValue::as_str).map(str::to_owned);
        let full_path = string("name").unwrap_or_default();
        let size = json
            .get("size")
            .and_then(JsonValue::as_str)
            .and_then(|size| size.parse().ok())
            .or_else(|| json.get("size").and_then(JsonValue::as_u64));
        let custom = json.get("metadata").and_then(JsonValue::as_object).map(
            |map| -> BTreeMap<String, String> {
                map.iter()
                    .filter_map(|(key, value)| {
                        value.as_str().map(|value| (key.clone(), value.to_owned()))
                    })
                    .collect()
            },
        );
        ObjectMetadata {
            bucket: string("bucket"),
            generation: string("generation"),
            metageneration: string("metageneration"),
            name: last_component(&full_path),
            full_path,
            size,
            time_created: string("timeCreated"),
            updated: string("updated"),
            md5_hash: string("md5Hash"),
            cache_control: string("cacheControl"),
            content_disposition: string("contentDisposition"),
            content_encoding: string("contentEncoding"),
            content_language: string("contentLanguage"),
            content_type: string("contentType"),
            custom_metadata: custom,
            download_tokens: string("downloadTokens"),
        }
    }
}

/// The metadata a caller may set: upload's optional argument, and what
/// `update_metadata` patches — the writable mappings only.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SettableMetadata {
    pub cache_control: Option<String>,
    pub content_disposition: Option<String>,
    pub content_encoding: Option<String>,
    pub content_language: Option<String>,
    pub content_type: Option<String>,
    pub md5_hash: Option<String>,
    pub custom_metadata: Option<BTreeMap<String, String>>,
}

impl SettableMetadata {
    /// The fields set, as the server resource JSON (`toResourceString`):
    /// only writable mappings, only when present.
    pub fn resource(&self) -> JsonValue {
        let mut resource = serde_json::Map::new();
        let mut put = |key: &str, value: &Option<String>| {
            if let Some(value) = value {
                resource.insert(key.to_owned(), JsonValue::String(value.clone()));
            }
        };
        put("cacheControl", &self.cache_control);
        put("contentDisposition", &self.content_disposition);
        put("contentEncoding", &self.content_encoding);
        put("contentLanguage", &self.content_language);
        put("contentType", &self.content_type);
        put("md5Hash", &self.md5_hash);
        if let Some(custom) = &self.custom_metadata {
            resource.insert(
                "metadata".to_owned(),
                JsonValue::Object(
                    custom
                        .iter()
                        .map(|(key, value)| (key.clone(), JsonValue::String(value.clone())))
                        .collect(),
                ),
            );
        }
        JsonValue::Object(resource)
    }
}

/// A list page: the object paths under the prefix, the synthetic folder
/// prefixes, and the token for the next page.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ListResult {
    pub prefixes: Vec<String>,
    pub items: Vec<String>,
    pub next_page_token: Option<String>,
}

impl ListResult {
    /// Parses a list response; a missing `items` is no objects. The
    /// `prefixes` are plain strings, the items' `name` is their path.
    pub fn parse(json: &JsonValue) -> ListResult {
        let strings = |key: &str| -> Vec<String> {
            json.get(key)
                .and_then(JsonValue::as_array)
                .map(|values| {
                    values
                        .iter()
                        .filter_map(JsonValue::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default()
        };
        ListResult {
            prefixes: strings("prefixes"),
            items: json
                .get("items")
                .and_then(JsonValue::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.get("name").and_then(JsonValue::as_str))
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default(),
            next_page_token: json
                .get("nextPageToken")
                .and_then(JsonValue::as_str)
                .map(str::to_owned),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn encoding_matches_uri_encode_component() {
        assert_eq!(encode_component("a/b c.d"), "a%2Fb%20c.d");
        assert_eq!(encode_component("yes!~*'()"), "yes!~*'()");
        assert_eq!(encode_component("plain-file_1.0"), "plain-file_1.0");
        // UTF-8 goes out as its bytes, upper-case hex, like JS.
        assert_eq!(encode_component("é"), "%C3%A9");
    }

    #[test]
    fn urls_follow_the_sdk_shapes() {
        assert_eq!(
            objects_base("http://127.0.0.1:19199", "demo-nsh.firebasestorage.app"),
            "http://127.0.0.1:19199/v0/b/demo-nsh.firebasestorage.app/o"
        );
        assert_eq!(
            start_url("http://127.0.0.1:19199/v0/b/b/o", "dir/file.txt"),
            "http://127.0.0.1:19199/v0/b/b/o?name=dir%2Ffile.txt"
        );
        assert_eq!(
            object_url("http://127.0.0.1:19199/v0/b/b/o", "dir/file.txt"),
            "http://127.0.0.1:19199/v0/b/b/o/dir%2Ffile.txt"
        );
    }

    #[test]
    fn origin_takes_the_emulator_switch_then_the_real_host() {
        let mut options = FirebaseOptions::demo();
        assert_eq!(origin(&options).unwrap(), "http://127.0.0.1:19199");
        options.emulator.storage = None;
        assert_eq!(
            origin(&options).unwrap(),
            "https://firebasestorage.googleapis.com"
        );
        options.storage_bucket = None;
        let error = origin(&options).unwrap_err();
        assert_eq!(error.code, NO_DEFAULT_BUCKET);
    }

    #[test]
    fn metadata_parses_and_builds_the_download_url() {
        let metadata = ObjectMetadata::parse(&json!({
            "name": "dir/file.txt",
            "bucket": "demo-nsh.firebasestorage.app",
            "generation": "1",
            "metageneration": "1",
            "size": "11",
            "contentType": "text/plain",
            "downloadTokens": "tok1,tok2",
            "metadata": {"origin": "unit-test"},
        }));
        assert_eq!(metadata.full_path, "dir/file.txt");
        assert_eq!(metadata.name, "file.txt");
        assert_eq!(metadata.size, Some(11));
        assert_eq!(metadata.content_type.as_deref(), Some("text/plain"));
        assert_eq!(
            metadata
                .custom_metadata
                .as_ref()
                .map(|m| m["origin"].as_str()),
            Some("unit-test")
        );
        assert_eq!(
            metadata.download_url("http://127.0.0.1:19199"),
            Some(
                "http://127.0.0.1:19199/v0/b/demo-nsh.firebasestorage.app/o/dir%2Ffile.txt?alt=media&token=tok1"
                    .to_owned()
            )
        );
        let no_tokens = ObjectMetadata::parse(&json!({ "name": "a" }));
        assert_eq!(no_tokens.download_url("http://127.0.0.1:19199"), None);
    }

    #[test]
    fn settable_metadata_serializes_writable_fields_only() {
        let mut custom = BTreeMap::new();
        custom.insert("who".to_owned(), "test".to_owned());
        let metadata = SettableMetadata {
            content_type: Some("text/tab-separated-values".to_owned()),
            cache_control: Some("public,max-age=60".to_owned()),
            custom_metadata: Some(custom),
            ..SettableMetadata::default()
        };
        assert_eq!(
            metadata.resource(),
            json!({
                "cacheControl": "public,max-age=60",
                "contentType": "text/tab-separated-values",
                "metadata": {"who": "test"},
            })
        );
        // The size and the name are not caller-settable; the upload start
        // body adds the name separately (writable `name` mapping).
        assert!(metadata.resource().get("size").is_none());
    }

    #[test]
    fn list_pages_parse() {
        // The server sends prefixes as strings, items as name objects.
        let result = ListResult::parse(&json!({
            "prefixes": ["probe/inner/"],
            "items": [{"name": "probe/a.txt"}, {"name": "probe/b.txt"}],
            "nextPageToken": "next-1",
        }));
        assert_eq!(result.prefixes, vec!["probe/inner/".to_owned()]);
        assert_eq!(
            result.items,
            vec!["probe/a.txt".to_owned(), "probe/b.txt".to_owned()]
        );
        assert_eq!(result.next_page_token.as_deref(), Some("next-1"));

        let empty = ListResult::parse(&json!({ "prefixes": [] }));
        assert_eq!(empty, ListResult::default());
    }
}
