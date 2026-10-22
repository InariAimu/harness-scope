//! Stream complete logs while skipping transcript bodies during JSON decoding.
//! Every byte is still hashed and every row validated; usage counters remain intact.
use anyhow::{Context, Result};
use serde::de::{DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fmt,
    fs::File,
    io::{BufRead, BufReader, Read},
    path::Path,
};

#[derive(Clone, Copy)]
enum Projection {
    All,
    Fields(&'static [(&'static str, Projection)]),
}
use Projection::{All, Fields};
const ITEM: Projection = Fields(&[("type", All)]);
const SOURCE: Projection = Fields(&[(
    "subagent",
    Fields(&[("thread_spawn", Fields(&[("parent_thread_id", All)]))]),
)]);
const PAYLOAD: Projection = Fields(&[
    ("effort", All),
    ("service_tier", All),
    (
        "thread_settings",
        Fields(&[("model", All), ("service_tier", All)]),
    ),
    ("model_provider", All),
    ("id", All),
    ("thread_id", All),
    ("turn_id", All),
    ("threadId", All),
    ("response_id", All),
    ("history_mode", All),
    ("history_base", Fields(&[("thread_id", All)])),
    ("rate_limits", Fields(&[("limit_id", All)])),
    ("type", All),
    ("model", All),
    ("role", All),
    ("forked_from_id", All),
    ("parent_thread_id", All),
    ("source", SOURCE),
    ("info", All),
    ("usage", All),
    ("started_at_ms", All),
    ("item", ITEM),
]);
const CODEX: Projection = Fields(&[("type", All), ("timestamp", All), ("payload", PAYLOAD)]);
const CLAUDE: Projection = Fields(&[
    ("type", All),
    ("timestamp", All),
    ("sessionId", All),
    ("id", All),
    ("requestRef", All),
    ("effort", All),
    ("perTurnEffort", All),
    (
        "params",
        Fields(&[
            ("model", All),
            ("output_config", Fields(&[("effort", All)])),
        ]),
    ),
    (
        "message",
        Fields(&[
            ("id", All),
            ("model", All),
            ("usage", All),
            ("stop_reason", All),
        ]),
    ),
]);

impl<'de> DeserializeSeed<'de> for Projection {
    type Value = Value;
    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> Result<Value, D::Error> {
        match self {
            All => serde::Deserialize::deserialize(d),
            Fields(_) => d.deserialize_any(self),
        }
    }
}
impl<'de> Visitor<'de> for Projection {
    type Value = Value;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a JSON value")
    }
    fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Value, M::Error> {
        let Fields(fields) = self else { unreachable!() };
        let mut out = serde_json::Map::new();
        while let Some(key) = map.next_key::<String>()? {
            if let Some((_, projection)) = fields.iter().find(|(name, _)| *name == key) {
                out.insert(key, map.next_value_seed(*projection)?);
            } else {
                // Decode skipped strings too: IgnoredAny would accept lone UTF-16
                // surrogates that the complete Value reader correctly rejects.
                map.next_value_seed(Fields(&[]))?;
            }
        }
        Ok(Value::Object(out))
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
        while seq.next_element_seed(Fields(&[]))?.is_some() {}
        Ok(Value::Null)
    }
    fn visit_str<E: serde::de::Error>(self, _: &str) -> Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_bool<E: serde::de::Error>(self, _: bool) -> Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_i64<E: serde::de::Error>(self, _: i64) -> Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_u64<E: serde::de::Error>(self, _: u64) -> Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_f64<E: serde::de::Error>(self, _: f64) -> Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_unit<E: serde::de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }
}

pub fn read(path: &Path, codex: bool) -> Result<(Vec<Value>, String, Vec<String>, bool)> {
    let file = File::open(path)?;
    let input: Box<dyn Read> =
        if path.to_string_lossy().ends_with(".zst") || path.to_string_lossy().ends_with(".zstd") {
            Box::new(zstd::stream::read::Decoder::new(file)?)
        } else {
            Box::new(file)
        };
    let mut reader = BufReader::new(input);
    let mut line = String::new();
    let mut hash = Sha256::new();
    let mut rows = Vec::new();
    let mut errors = Vec::new();
    let mut number = 0;
    loop {
        line.clear();
        if reader
            .read_line(&mut line)
            .context("Read UTF-8 usage log")?
            == 0
        {
            break;
        }
        number += 1;
        hash.update(line.as_bytes());
        if line.trim().is_empty() {
            continue;
        }
        let mut de = serde_json::Deserializer::from_str(&line);
        let result = (if codex { CODEX } else { CLAUDE })
            .deserialize(&mut de)
            .and_then(|v| {
                de.end()?;
                Ok(v)
            });
        match result {
            Ok(mut v) => {
                if codex {
                    if let Some(object) = v.as_object_mut() {
                        object.insert("_scopeLine".into(), number.into());
                    }
                }
                rows.push(v)
            }
            Err(e) => errors.push(format!(
                "{}:{number}: {}{e}",
                path.display(),
                if line.ends_with('\n') {
                    "Invalid JSON: "
                } else {
                    "Incomplete tail: "
                }
            )),
        }
    }
    let complete = errors.is_empty();
    Ok((rows, format!("{:x}", hash.finalize()), errors, complete))
}
