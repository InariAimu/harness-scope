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
    Loose(&'static [(&'static str, Projection)]),
    List(&'static Projection),
    Skip,
}
use Projection::{All, Fields, List, Loose, Skip};
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
    (
        "history_base",
        Fields(&[
            ("thread_id", All),
            ("end_ordinal_exclusive", All),
            ("end_byte_offset", All),
        ]),
    ),
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
const OPENCODE_TIME: Projection = Loose(&[
    ("created", All),
    ("start", All),
    ("completed", All),
    ("streamed", All),
]);
const OPENCODE_PART: Projection = Loose(&[("type", All), ("time", OPENCODE_TIME)]);
const OPENCODE: Projection = Projection::Loose(&[
    ("role", All),
    ("time", OPENCODE_TIME),
    (
        "tokens",
        Loose(&[
            ("input", All),
            ("output", All),
            ("reasoning", All),
            ("cache", Loose(&[("read", All), ("write", All)])),
        ]),
    ),
    ("cost", All),
    ("modelID", All),
    ("providerID", All),
    (
        "model",
        Projection::Loose(&[("id", All), ("providerID", All)]),
    ),
    ("content", List(&OPENCODE_PART)),
]);

/// Extract accounting fields while treating unrelated transcript text as opaque.
/// JSON structure and every selected field are still validated by serde_json.
pub(super) fn opencode(data: &str, part: bool) -> Result<Value> {
    let mut de = serde_json::Deserializer::from_str(data);
    let value = (if part { OPENCODE_PART } else { OPENCODE }).deserialize(&mut de)?;
    de.end()?;
    Ok(value)
}

/// Validate a plain Codex parent's exact physical prefix without retaining
/// transcript bodies. Both byte and line boundaries must agree.
pub(super) fn codex_prefix(path: &Path, bytes: u64, ordinal: u64) -> Result<(Vec<Value>, String)> {
    anyhow::ensure!(
        !path.to_string_lossy().ends_with(".zst") && !path.to_string_lossy().ends_with(".zstd"),
        "Compressed Codex parent boundary is unavailable"
    );
    let before = crate::scan_cache::Stamp::read(path)?;
    let file = File::open(path)?;
    anyhow::ensure!(
        file.metadata()?.len() >= bytes,
        "Codex parent is shorter than its history boundary"
    );
    let mut reader = BufReader::new(file);
    let mut line = String::new();
    let mut hash = Sha256::new();
    let mut events = Vec::new();
    let mut position = 0u64;
    let mut count = 0u64;
    while position < bytes {
        line.clear();
        let length = reader.read_line(&mut line)? as u64;
        anyhow::ensure!(
            length > 0 && line.ends_with('\n'),
            "Incomplete Codex parent history line"
        );
        position = position
            .checked_add(length)
            .context("Codex parent boundary overflow")?;
        anyhow::ensure!(
            position <= bytes,
            "Codex parent byte boundary splits a line"
        );
        count += 1;
        hash.update(line.as_bytes());
        let mut de = serde_json::Deserializer::from_str(&line);
        let event = CODEX.deserialize(&mut de)?;
        de.end()?;
        events.push(event);
    }
    anyhow::ensure!(
        position == bytes && count == ordinal,
        "Codex parent history byte/ordinal mismatch"
    );
    anyhow::ensure!(
        crate::scan_cache::Stamp::read(path)? == before,
        "Codex parent changed while validating history boundary"
    );
    Ok((events, format!("{:x}", hash.finalize())))
}

impl<'de> DeserializeSeed<'de> for Projection {
    type Value = Value;
    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> Result<Value, D::Error> {
        match self {
            All => serde::Deserialize::deserialize(d),
            Skip => {
                <serde::de::IgnoredAny as serde::Deserialize>::deserialize(d)?;
                Ok(Value::Null)
            }
            Fields(_) | Loose(_) | List(_) => d.deserialize_any(self),
        }
    }
}
impl<'de> Visitor<'de> for Projection {
    type Value = Value;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a JSON value")
    }
    fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Value, M::Error> {
        let (fields, loose) = match self {
            Fields(fields) => (fields, false),
            Projection::Loose(fields) => (fields, true),
            _ => (&[][..], true),
        };
        let mut out = serde_json::Map::new();
        while let Some(key) = map.next_key::<String>()? {
            if let Some((_, projection)) = fields.iter().find(|(name, _)| *name == key) {
                out.insert(key, map.next_value_seed(*projection)?);
            } else {
                // Codex/Claude retain strict whole-row validation; OpenCode usage
                // skips unrelated transcript text while validating selected fields.
                map.next_value_seed(if loose { Skip } else { Fields(&[]) })?;
            }
        }
        Ok(Value::Object(out))
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
        if let Projection::List(element) = self {
            let mut values = Vec::new();
            while let Some(value) = seq.next_element_seed(*element)? {
                values.push(value);
            }
            Ok(Value::Array(values))
        } else {
            let skip = if matches!(self, Projection::Loose(_)) {
                Projection::Skip
            } else {
                Fields(&[])
            };
            while seq.next_element_seed(skip)?.is_some() {}
            Ok(Value::Null)
        }
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
