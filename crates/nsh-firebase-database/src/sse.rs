//! The Realtime Database's Server-Sent Events stream: the frame format
//! of <https://firebase.google.com/docs/database/rest/retrieve-data>
//! ("Streaming from the REST API") — `event:`/`data:` lines, the events
//! `put`, `patch`, `keep-alive`, `cancel` and `auth_revoked`, and the
//! `{path, data}` payload — and the listener cache those events apply
//! to. A stream can break a frame across chunks, so the parser keeps
//! bytes and splits on whole delimiters only.

use serde_json::Value as JsonValue;

/// One event of the stream.
#[derive(Debug, Clone, PartialEq)]
pub enum SseEvent {
    /// Replace the cache at `path` with `data` (a `null` data deletes).
    Put { path: String, data: JsonValue },
    /// Merge each key of `data` into the cache at `path`.
    Patch { path: String, data: JsonValue },
    /// The connection is alive; nothing to apply.
    KeepAlive,
    /// Security rules no longer allow the read: stop, do not reconnect.
    Cancel,
    /// The `auth` credential stopped being valid: stop and re-authenticate.
    AuthRevoked,
}

/// A frame-splitting parser: fed every chunk of the stream as it
/// arrives, it gives the complete events.
#[derive(Default)]
pub struct FrameParser {
    buffer: Vec<u8>,
}

impl FrameParser {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feeds one chunk of body bytes, giving every event a complete
    /// frame carries. Chunks split anywhere, including inside a frame or
    /// a multi-byte character.
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<SseEvent> {
        self.buffer.extend_from_slice(chunk);
        let mut events = Vec::new();
        while let Some(end) = frame_end(&self.buffer) {
            let frame: Vec<u8> = self.buffer.drain(..end + 1).collect();
            if let Some(event) = parse_frame(&frame) {
                events.push(event);
            }
        }
        events
    }
}

/// Where a frame ends and its delimiter does: the earliest `\n\n` or
/// `\r\n\r\n`. Frames end past the first newline of the delimiter.
fn frame_end(buffer: &[u8]) -> Option<usize> {
    let mut at = 0;
    while at + 1 < buffer.len() {
        if buffer[at] == b'\n' && buffer[at + 1] == b'\n' {
            return Some(at + 1);
        }
        if buffer[at] == b'\r'
            && buffer[at + 1] == b'\n'
            && buffer.get(at + 2) == Some(&b'\r')
            && buffer.get(at + 3) == Some(&b'\n')
        {
            return Some(at + 3);
        }
        at += 1;
    }
    None
}

/// One frame's event: `event:` and `data:` lines (`:` comments are
/// keep-alives of another kind). An unknown event or an unreadable
/// payload is ignored, as the format's forward compatibility asks.
fn parse_frame(frame: &[u8]) -> Option<SseEvent> {
    let text = String::from_utf8_lossy(frame);
    let mut name = String::new();
    let mut data = String::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix(':') {
            let _ = rest;
            continue;
        }
        if let Some(rest) = line.strip_prefix("event:") {
            name = rest.trim_start_matches(' ').to_owned();
        } else if let Some(rest) = line.strip_prefix("data:") {
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(rest.trim_start_matches(' '));
        }
    }
    match name.as_str() {
        "put" | "patch" => {
            let payload: JsonValue = serde_json::from_str(&data).ok()?;
            let path = payload.get("path")?.as_str()?.to_owned();
            let data = payload.get("data").cloned()?;
            Some(if name == "put" {
                SseEvent::Put { path, data }
            } else {
                SseEvent::Patch { path, data }
            })
        }
        "keep-alive" => Some(SseEvent::KeepAlive),
        "cancel" => Some(SseEvent::Cancel),
        "auth_revoked" => Some(SseEvent::AuthRevoked),
        _ => None,
    }
}

/// Applies `event` to the listener's cache: the value of the ref the
/// stream listens at. `put` at `/` replaces it whole (that is how a
/// reconnect resyncs), `put` deeper sets one path (a `null` deletes),
/// `patch` merges keys.
pub fn apply_event(cache: &mut Option<JsonValue>, event: &SseEvent) {
    match event {
        SseEvent::Put { path, data } => set_at(cache, path, data.clone()),
        SseEvent::Patch { path, data } => {
            let Some(entries) = data.as_object() else {
                return;
            };
            for (key, value) in entries {
                set_at(cache, &format!("{path}/{key}"), value.clone());
            }
        }
        SseEvent::KeepAlive | SseEvent::Cancel | SseEvent::AuthRevoked => {}
    }
}

/// Sets `value` at `path` (relative to the listen URL), creating the
/// objects along it (a scalar in the way becomes an object); a `null`
/// value removes what is there.
fn set_at(cache: &mut Option<JsonValue>, path: &str, value: JsonValue) {
    let parts: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    if parts.is_empty() {
        *cache = (value != JsonValue::Null).then_some(value);
        return;
    }
    if !cache.as_ref().is_some_and(JsonValue::is_object) {
        *cache = Some(JsonValue::Object(Default::default()));
    }
    let mut at = cache.as_mut().expect("root just made");
    for part in &parts[..parts.len() - 1] {
        at = object_at(at)
            .entry(part.to_string())
            .or_insert_with(|| JsonValue::Object(Default::default()));
    }
    let object = object_at(at);
    if value == JsonValue::Null {
        object.remove(parts[parts.len() - 1]);
    } else {
        object.insert(parts[parts.len() - 1].to_owned(), value);
    }
}

/// `at` as an object, a scalar in the way becoming an empty one.
fn object_at(at: &mut JsonValue) -> &mut serde_json::Map<String, JsonValue> {
    if !at.is_object() {
        *at = JsonValue::Object(Default::default());
    }
    at.as_object_mut().expect("an object just set")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_chunk_of_two_frames_gives_two_events() {
        let mut parser = FrameParser::new();
        let events = parser.feed(
            b"event: put\ndata: {\"path\":\"/\",\"data\":{\"a\":1}}\n\n\
              event: put\ndata: {\"path\":\"/b\",\"data\":2}\n\n",
        );
        assert_eq!(
            events,
            vec![
                SseEvent::Put {
                    path: "/".to_owned(),
                    data: json!({"a": 1}),
                },
                SseEvent::Put {
                    path: "/b".to_owned(),
                    data: json!(2),
                },
            ]
        );
    }

    #[test]
    fn a_frame_split_across_chunks_parses_when_whole() {
        let mut parser = FrameParser::new();
        assert!(parser.feed(b"event: put\ndata: {\"path\":").is_empty());
        assert!(parser.feed(b"\"/x\",\"data\":true}").is_empty());
        let events = parser.feed(b"\r\n\r\n");
        assert_eq!(
            events,
            vec![SseEvent::Put {
                path: "/x".to_owned(),
                data: json!(true),
            }]
        );
    }

    #[test]
    fn a_multi_byte_character_split_across_chunks_survives() {
        let mut parser = FrameParser::new();
        let frame = format!(
            "event: put\ndata: {}\n\n",
            json!({"path": "/name", "data": "育苗"})
        );
        let bytes = frame.as_bytes();
        let split = 16;
        let mut events = parser.feed(&bytes[..split]);
        events.extend(parser.feed(&bytes[split..]));
        assert_eq!(
            events,
            vec![SseEvent::Put {
                path: "/name".to_owned(),
                data: json!("育苗"),
            }]
        );
    }

    #[test]
    fn keep_alive_cancel_and_auth_revoked_parse() {
        let mut parser = FrameParser::new();
        let events = parser.feed(
            "event: keep-alive\ndata: null\n\n\
             :this is a comment\n\n\
             event: cancel\ndata: null\n\n\
             event: auth_revoked\ndata: \"Expired auth token\"\n\n"
                .as_bytes(),
        );
        assert_eq!(
            events,
            vec![SseEvent::KeepAlive, SseEvent::Cancel, SseEvent::AuthRevoked,]
        );
    }

    #[test]
    fn an_unknown_event_and_an_unreadable_payload_are_ignored() {
        let mut parser = FrameParser::new();
        let events = parser.feed(
            b"event: server_tick\ndata: {\"now\":1}\n\n\
              event: put\ndata: not-json\n\n\
              event: put\ndata: {\"path\":\"/ok\",\"data\":7}\n\n",
        );
        assert_eq!(
            events,
            vec![SseEvent::Put {
                path: "/ok".to_owned(),
                data: json!(7),
            }]
        );
    }

    #[test]
    fn a_put_at_root_replaces_the_whole_cache() {
        let mut cache = Some(json!({"a": 1, "b": 2}));
        apply_event(
            &mut cache,
            &SseEvent::Put {
                path: "/".to_owned(),
                data: json!({"c": 3}),
            },
        );
        assert_eq!(cache, Some(json!({"c": 3})));
        apply_event(
            &mut cache,
            &SseEvent::Put {
                path: "/".to_owned(),
                data: json!(null),
            },
        );
        assert_eq!(cache, None);
    }

    #[test]
    fn a_deep_put_creates_its_parents_and_a_null_deletes() {
        let mut cache = None;
        apply_event(
            &mut cache,
            &SseEvent::Put {
                path: "/todos/one/title".to_owned(),
                data: json!("Walk"),
            },
        );
        assert_eq!(cache, Some(json!({"todos": {"one": {"title": "Walk"}}})));
        apply_event(
            &mut cache,
            &SseEvent::Put {
                path: "/todos/one/title".to_owned(),
                data: json!(null),
            },
        );
        assert_eq!(cache, Some(json!({"todos": {"one": {}}})));
    }

    #[test]
    fn a_patch_merges_its_keys_at_its_path() {
        // The docs' example: patch /c with {foo: 3, baz: 4} keeps bar.
        let mut cache = Some(json!({"c": {"foo": 1, "bar": 2}}));
        apply_event(
            &mut cache,
            &SseEvent::Patch {
                path: "/c".to_owned(),
                data: json!({"foo": 3, "baz": 4}),
            },
        );
        assert_eq!(cache, Some(json!({"c": {"foo": 3, "bar": 2, "baz": 4}})));
    }
}
