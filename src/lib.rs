use serde::Serialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Read, Write};
use std::net::{Ipv4Addr, SocketAddrV4, TcpStream};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, Debug)]
pub struct FrameRef(u64);

impl FrameRef {
    pub fn disabled() -> Self { Self(u64::MAX) }
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
        value: normalize(serde_json::to_value(value).expect("algo-vis: value could not be serialized"), rust_type),
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
            let tag = if hint.contains("HashSet<") || hint.contains("BTreeSet<") { "set" } else { "array" };
            let mut items = items.into_iter().map(|item| normalize(item, "")).collect::<Vec<_>>();
            if tag == "set" { items.sort_by_key(Value::to_string); }
            json!({"t":tag,"items":items})
        }
        Value::Object(fields) if hint.contains("HashMap<") || hint.contains("BTreeMap<") => {
            let key_hint = hint.split('<').nth(1).unwrap_or("").split(',').next().unwrap_or("").trim();
            let entries: Vec<Value> = fields.into_iter().map(|(key,value)| {
                let typed_key = if is_integer_type(key_hint) {
                    if key.parse::<i128>().is_err() && key.parse::<u128>().is_err() { panic!("algo-vis: invalid integer map key") }
                    json!({"t":"int","v":key})
                } else if key_hint == "bool" {
                    json!({"t":"bool","v":key == "true"})
                } else {
                    json!({"t":"string","v":key})
                };
                json!({"key":typed_key,"value":normalize(value, "")})
            }).collect();
            json!({"t":"map","entries":entries})
        }
        Value::Object(fields) => json!({"t":"record","fields":fields.into_iter().map(|(name,value)| {
            json!({"name":name,"value":normalize(value, "")})
        }).collect::<Vec<_>>() }),
    }
}

fn is_integer_type(name: &str) -> bool {
    matches!(name, "i8"|"i16"|"i32"|"i64"|"i128"|"isize"|"u8"|"u16"|"u32"|"u64"|"u128"|"usize")
}

fn is_integer_number(number: &serde_json::Number) -> bool {
    !number.to_string().bytes().any(|byte| matches!(byte, b'.'|b'e'|b'E'))
}

pub fn span_segment<T: Serialize + ?Sized>(value: &T) -> Value {
    let raw = serde_json::to_value(value).expect("algo-vis: span ID could not be serialized");
    let kind = match &raw {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(n) if is_integer_number(n) => "int",
        Value::Number(_) => "float",
        Value::String(_) => "string",
        _ => panic!("algo-vis: span ID must be a scalar"),
    };
    json!({"t": kind, "v": if raw.is_number() { Value::String(raw.to_string()) } else { raw }})
}

struct Recorder {
    next_seq: u64,
    run_id: String,
    values: BTreeMap<String, Value>,
    sink: Sink,
}

enum Sink { File(BufWriter<File>), Http(u16), Disabled }

static RECORDER: OnceLock<Option<Mutex<Recorder>>> = OnceLock::new();

fn recorder() -> Option<&'static Mutex<Recorder>> {
    RECORDER.get_or_init(|| {
        let sink = if let Some(path) = std::env::var_os("VIZ_TRACE_PATH") {
            let file = OpenOptions::new().create(true).append(true).open(path)
                .expect("algo-vis: cannot open trace file");
            Sink::File(BufWriter::new(file))
        } else {
            let port = std::env::var("VIZ_PORT").ok().and_then(|text| text.parse::<u16>().ok()).unwrap_or(4317);
            let address = SocketAddrV4::new(Ipv4Addr::LOCALHOST, port);
            if TcpStream::connect_timeout(&address.into(), Duration::from_millis(100)).is_err() { return None; }
            Sink::Http(port)
        };
        let millis = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis();
        let generated_id = format!("run-{millis}-{}", std::process::id());
        let run_id = std::env::var("VIZ_RUN_ID").ok()
            .filter(|id| !id.is_empty() && id.len() <= 128 && id.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'))
            .unwrap_or(generated_id);
        Some(Mutex::new(Recorder { next_seq: 0, run_id, values: BTreeMap::new(), sink }))
    }).as_ref()
}

pub fn enabled() -> bool { recorder().is_some() }

pub fn record(span: Vec<Value>, from: Option<FrameRef>, fields: Vec<Field>, source: (&str, u32)) -> FrameRef {
    let mut state = recorder().expect("algo-vis: recorder is disabled").lock().expect("algo-vis: recorder lock poisoned");
    let seq = state.next_seq;
    if let Some(parent) = from {
        assert!(parent.0 < seq, "algo-vis: from must refer to an earlier frame in this run");
    }
    let mut observed = BTreeMap::new();
    for field in fields { observed.insert(field.name, field); }
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
    if let Some(parent) = from { event["from"] = json!(parent.0.to_string()); }
    if seq == 0 {
        event["values"] = Value::Array(state.values.values().cloned().collect());
    } else {
        event["ops"] = Value::Array(ops);
    }
    let body = serde_json::to_vec(&event).expect("algo-vis: event encode failed");
    match &mut state.sink {
        Sink::File(writer) => {
            writer.write_all(&body).expect("algo-vis: trace write failed");
            writer.write_all(b"\n").expect("algo-vis: trace write failed");
            writer.flush().expect("algo-vis: trace flush failed");
        }
        Sink::Http(port) => {
            let address = SocketAddrV4::new(Ipv4Addr::LOCALHOST, *port);
            let sent = TcpStream::connect_timeout(&address.into(), Duration::from_millis(200))
                .and_then(|mut stream| {
                    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
                    stream.write_all(format!("POST /api/record HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).as_bytes())?;
                    stream.write_all(&body)?;
                    let mut response = String::new();
                    stream.read_to_string(&mut response)?;
                    if !response.starts_with("HTTP/1.1 200") { return Err(std::io::Error::other("algo-vis: server rejected frame")); }
                    Ok(())
                });
            if let Err(error) = sent {
                eprintln!("algo-vis: recording stopped: {error}");
                state.sink = Sink::Disabled;
            }
        }
        Sink::Disabled => {}
    }
    state.next_seq += 1;
    FrameRef(seq)
}

#[doc(hidden)]
#[macro_export]
macro_rules! __record_value {
    ($name:ident) => { &$name };
    ($name:ident = $value:expr) => { &$value };
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
        $crate::__record_impl!([$($span),*], Some($from), $($name $(= $value)?),+)
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
        assert_eq!(field("wide", &i128::MAX).value, json!({"t":"int","v":i128::MAX.to_string()}));
        let map = BTreeMap::from([(1_i32, 9_i32)]);
        assert_eq!(field("map", &map).value, json!({"t":"map","entries":[{
            "key":{"t":"int","v":"1"},"value":{"t":"int","v":"9"}
        }]}));
    }
}
