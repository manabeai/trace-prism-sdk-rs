use serde::ser::{SerializeMap, Serializer};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, Debug)]
pub struct FrameRef(u64);

impl FrameRef {
    pub fn disabled() -> Self {
        Self(u64::MAX)
    }
}

impl Serialize for FrameRef {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // Distinguishes a frame handle from a user ID in the generic `from:` macro argument.
        let mut map = serializer.serialize_map(Some(1))?;
        map.serialize_entry("$tracePrismFrameRef", &self.0)?;
        map.end()
    }
}

pub enum Origin {
    Frame(FrameRef),
    Id(Vec<Value>),
}

pub struct Field {
    name: &'static str,
    rust_type: &'static str,
    value: Value,
}

pub fn field<T: Serialize + ?Sized>(name: &'static str, value: &T) -> Field {
    let rust_type = std::any::type_name::<T>();
    Field {
        name,
        rust_type,
        value: normalize(
            serde_json::to_value(value).expect("TracePrism: value could not be serialized"),
            rust_type,
        ),
    }
}

fn normalize(value: Value, hint: &str) -> Value {
    match value {
        Value::Null => json!({"t":"null"}),
        Value::Bool(v) => json!({"t":"bool","v":v}),
        Value::Number(v) if is_integer_number(&v) => json!({"t":"int","v":v.to_string()}),
        Value::Number(v) => json!({"t":"float","v":v.to_string()}),
        Value::String(v) => json!({"t":"string","v":v}),
        Value::Array(items) => {
            let tag = if hint.contains("HashSet<") || hint.contains("BTreeSet<") {
                "set"
            } else {
                "array"
            };
            let mut items = items
                .into_iter()
                .map(|item| normalize(item, ""))
                .collect::<Vec<_>>();
            if tag == "set" {
                items.sort_by_key(Value::to_string);
            }
            json!({"t":tag,"items":items})
        }
        Value::Object(fields) if hint.contains("HashMap<") || hint.contains("BTreeMap<") => {
            let key_hint = hint
                .split('<')
                .nth(1)
                .unwrap_or("")
                .split(',')
                .next()
                .unwrap_or("")
                .trim();
            let entries: Vec<Value> = fields
                .into_iter()
                .map(|(key, value)| {
                    let typed_key = if is_integer_type(key_hint) {
                        if key.parse::<i128>().is_err() && key.parse::<u128>().is_err() {
                            panic!("TracePrism: invalid integer map key")
                        }
                        json!({"t":"int","v":key})
                    } else if key_hint == "bool" {
                        json!({"t":"bool","v":key == "true"})
                    } else {
                        json!({"t":"string","v":key})
                    };
                    json!({"key":typed_key,"value":normalize(value, "")})
                })
                .collect();
            json!({"t":"map","entries":entries})
        }
        Value::Object(fields) => {
            json!({"t":"record","fields":fields.into_iter().map(|(name,value)| {
            json!({"name":name,"value":normalize(value, "")})
        }).collect::<Vec<_>>() })
        }
    }
}

fn is_integer_type(name: &str) -> bool {
    matches!(
        name,
        "i8" | "i16"
            | "i32"
            | "i64"
            | "i128"
            | "isize"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "u128"
            | "usize"
    )
}

fn is_integer_number(number: &serde_json::Number) -> bool {
    !number
        .to_string()
        .bytes()
        .any(|byte| matches!(byte, b'.' | b'e' | b'E'))
}

fn normalize_segment(raw: Value) -> Value {
    let kind = match &raw {
        Value::Null => return json!({"t":"null"}),
        Value::Bool(_) => "bool",
        Value::Number(n) if is_integer_number(n) => "int",
        Value::Number(_) => "float",
        Value::String(_) => "string",
        _ => panic!("TracePrism: span ID must be a scalar"),
    };
    json!({"t": kind, "v": if raw.is_number() { Value::String(raw.to_string()) } else { raw }})
}

pub fn span_segment<T: Serialize + ?Sized>(value: &T) -> Value {
    normalize_segment(
        serde_json::to_value(value).expect("TracePrism: span ID could not be serialized"),
    )
}

pub fn origin<T: Serialize + ?Sized>(value: &T) -> Origin {
    let raw = serde_json::to_value(value).expect("TracePrism: from ID could not be serialized");
    if let Value::Object(fields) = &raw {
        if fields.len() == 1 {
            if let Some(seq) = fields.get("$tracePrismFrameRef").and_then(Value::as_u64) {
                return Origin::Frame(FrameRef(seq));
            }
        }
    }
    match raw {
        Value::Array(items) => Origin::Id(items.into_iter().map(normalize_segment).collect()),
        scalar => Origin::Id(vec![normalize_segment(scalar)]),
    }
}

struct Recorder {
    next_seq: u64,
    run_id: String,
    values: BTreeMap<String, Value>,
    sink: Option<BufWriter<File>>,
}

/// SDK とビューワが共有する実行履歴の保存先。
pub fn run_dir() -> io::Result<PathBuf> {
    if let Some(path) = std::env::var_os("TRACEPRISM_RUN_DIR") {
        let path = PathBuf::from(path);
        if path.is_absolute() {
            return Ok(path);
        }
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "TRACEPRISM_RUN_DIR must be absolute",
        ));
    }
    #[cfg(target_os = "windows")]
    let base = std::env::var_os("APPDATA")
        .or_else(|| std::env::var_os("LOCALAPPDATA"))
        .map(PathBuf::from);
    #[cfg(target_os = "macos")]
    let base = std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join("Library/Application Support"));
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")));
    base.map(|path| path.join("traceprism/runs"))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "user data directory is unavailable",
            )
        })
}

static RECORDER: OnceLock<Option<Mutex<Recorder>>> = OnceLock::new();

fn recorder() -> Option<&'static Mutex<Recorder>> {
    RECORDER
        .get_or_init(|| {
            let millis = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis();
            let generated_id = format!("run-{millis}-{}", std::process::id());
            let run_id = std::env::var("VIZ_RUN_ID")
                .ok()
                .filter(|id| {
                    !id.is_empty()
                        && id.len() <= 128
                        && id.bytes().all(|byte| {
                            byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'
                        })
                })
                .unwrap_or(generated_id);
            let path = match std::env::var_os("VIZ_TRACE_PATH") {
                Some(path) => PathBuf::from(path),
                None => match run_dir() {
                    Ok(dir) => dir.join(format!("{run_id}.jsonl")),
                    Err(error) => {
                        eprintln!("TracePrism: recording unavailable: {error}");
                        return None;
                    }
                },
            };
            let sink = (|| -> io::Result<BufWriter<File>> {
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent)?;
                }
                let file = OpenOptions::new().create(true).append(true).open(path)?;
                Ok(BufWriter::new(file))
            })();
            let sink = match sink {
                Ok(sink) => sink,
                Err(error) => {
                    eprintln!("TracePrism: recording unavailable: {error}");
                    return None;
                }
            };
            Some(Mutex::new(Recorder {
                next_seq: 0,
                run_id,
                values: BTreeMap::new(),
                sink: Some(sink),
            }))
        })
        .as_ref()
}

pub fn enabled() -> bool {
    recorder()
        .and_then(|recorder| recorder.lock().ok().map(|state| state.sink.is_some()))
        .unwrap_or(false)
}

pub fn record(
    span: Vec<Value>,
    from: Option<Origin>,
    fields: Vec<Field>,
    source: (&str, u32),
) -> FrameRef {
    let Some(recorder) = recorder() else {
        return FrameRef::disabled();
    };
    let Ok(mut state) = recorder.lock() else {
        return FrameRef::disabled();
    };
    if state.sink.is_none() {
        return FrameRef::disabled();
    }
    let seq = state.next_seq;
    if let Some(Origin::Frame(parent)) = &from {
        assert!(
            parent.0 < seq,
            "TracePrism: from must refer to an earlier frame in this run"
        );
    }
    let mut observed = BTreeMap::new();
    for field in fields {
        observed.insert(field.name, field);
    }
    let mut ops = Vec::new();
    for field in observed.into_values() {
        let next = json!({"name":field.name,"sourceType":field.rust_type,"value":field.value});
        if state.values.get(field.name) != Some(&next) {
            ops.push(json!({"op":"put","name":field.name,"sourceType":field.rust_type,"value":field.value}));
            state.values.insert(field.name.to_owned(), next);
        }
    }
    let mut event = json!({
        "format": "viz.trace/v2", "kind": if seq == 0 { "snapshot" } else { "patch" },
        "seq": seq.to_string(), "span": span,
        "source": {"file": source.0, "line": source.1},
        "runId": state.run_id, "pid": std::process::id(), "producer": "rust"
    });
    match from {
        Some(Origin::Frame(parent)) => event["from"] = json!(parent.0.to_string()),
        Some(Origin::Id(id)) => event["fromId"] = Value::Array(id),
        None => {}
    }
    if seq == 0 {
        event["values"] = Value::Array(state.values.values().cloned().collect());
    } else {
        event["ops"] = Value::Array(ops);
    }
    let body = serde_json::to_vec(&event).expect("TracePrism: event encode failed");
    if let Some(writer) = state.sink.as_mut() {
        if let Err(error) = writer
            .write_all(&body)
            .and_then(|()| writer.write_all(b"\n"))
            .and_then(|()| writer.flush())
        {
            eprintln!("TracePrism: recording stopped: {error}");
            state.sink = None;
        }
    }
    state.next_seq += 1;
    FrameRef(seq)
}

#[doc(hidden)]
#[macro_export]
macro_rules! __record_value {
    ($name:ident) => {
        &$name
    };
    ($name:ident = $value:expr) => {
        &$value
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __record_impl {
    ([$($span:expr),*], $from:expr, $($name:ident $(= $value:expr)?),+ ) => {{
        if $crate::enabled() {
            let __span = vec![$($crate::span_segment(&$span)),*];
            let __fields = vec![$($crate::field(stringify!($name), $crate::__record_value!($name $(= $value)?))),+];
            $crate::record(__span, $from, __fields, (file!(), line!()))
        } else { $crate::FrameRef::disabled() }
    }};
}

#[macro_export]
macro_rules! record {
    ([$($span:expr),* $(,)?], from: $from:expr, $($name:ident $(= $value:expr)?),+ $(,)?) => {
        $crate::__record_impl!([$($span),*], Some($crate::origin(&$from)), $($name $(= $value)?),+)
    };
    ([$($span:expr),* $(,)?], $($name:ident $(= $value:expr)?),+ $(,)?) => {
        $crate::__record_impl!([$($span),*], None, $($name $(= $value)?),+)
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn preserves_wide_integers_and_typed_map_keys() {
        assert_eq!(
            field("wide", &i128::MAX).value,
            json!({"t":"int","v":i128::MAX.to_string()})
        );
        let map = BTreeMap::from([(1_i32, 9_i32)]);
        assert_eq!(
            field("map", &map).value,
            json!({"t":"map","entries":[{
                "key":{"t":"int","v":"1"},"value":{"t":"int","v":"9"}
            }]})
        );
    }

    #[test]
    fn accepts_scalar_and_path_origins_while_preserving_frame_references() {
        match origin(&3_usize) {
            Origin::Id(id) => assert_eq!(id, vec![json!({"t":"int","v":"3"})]),
            Origin::Frame(_) => panic!("scalar origin was treated as a frame"),
        }
        match origin(&[1_usize, 2]) {
            Origin::Id(id) => assert_eq!(
                id,
                vec![json!({"t":"int","v":"1"}), json!({"t":"int","v":"2"})]
            ),
            Origin::Frame(_) => panic!("path origin was treated as a frame"),
        }
        match origin(&Option::<i32>::None) {
            Origin::Id(id) => assert_eq!(id, vec![json!({"t":"null"})]),
            Origin::Frame(_) => panic!("null origin was treated as a frame"),
        }
        match origin(&FrameRef(7)) {
            Origin::Frame(frame) => assert_eq!(frame.0, 7),
            Origin::Id(_) => panic!("frame reference was treated as an ID"),
        }
    }
}
