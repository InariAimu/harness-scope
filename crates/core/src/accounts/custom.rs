//! Local provider labels and bounded, single-request usage queries.
use super::*;
use rquickjs::{Context as JsContext, Runtime};
use std::time::Instant;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Settings {
    pub enabled: bool,
    pub base_url: String,
    pub script: String,
    pub timeout_seconds: u64,
}

pub(super) fn is_provider(id: &str) -> bool {
    id == "custom"
        || id
            .strip_prefix("custom:")
            .is_some_and(|v| uuid::Uuid::parse_str(v).is_ok())
}

fn base_url(value: &str) -> Result<reqwest::Url> {
    let url = reqwest::Url::parse(value).context("Invalid custom query URL")?;
    anyhow::ensure!(
        matches!(url.scheme(), "http" | "https")
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none(),
        "Custom query base URL must be HTTP(S), without credentials, query or fragment"
    );
    Ok(url)
}

pub(super) fn validate(settings: &Settings) -> Result<()> {
    anyhow::ensure!(
        (2..=30).contains(&settings.timeout_seconds),
        "Custom query timeout must be 2–30 seconds"
    );
    anyhow::ensure!(
        settings.base_url.len() <= 2048 && settings.script.len() <= 32768,
        "Custom query configuration is too large"
    );
    if settings.enabled {
        base_url(&settings.base_url)?;
        anyhow::ensure!(
            !settings.script.trim().is_empty(),
            "Custom query script is required"
        );
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    url: String,
    #[serde(default = "get")]
    method: String,
    #[serde(default)]
    headers: BTreeMap<String, String>,
    body: Option<Value>,
}
fn get() -> String {
    "GET".into()
}

/// Secrets are expanded in request strings *after* JavaScript evaluation. They are
/// never source-code substitutions, globals, exception text, or persisted results.
pub(super) fn request(
    settings: &Settings,
    key: &str,
    network: &crate::network::Settings,
) -> Result<Value> {
    validate(settings)?;
    anyhow::ensure!(settings.enabled, "Custom usage query is disabled");
    let base = base_url(&settings.base_url)?;
    let runtime = Runtime::new().context("Cannot initialize custom query runtime")?;
    runtime.set_memory_limit(16 * 1024 * 1024);
    runtime.set_max_stack_size(256 * 1024);
    let deadline = Instant::now() + Duration::from_millis(500);
    runtime.set_interrupt_handler(Some(Box::new(move || Instant::now() >= deadline)));
    let context = JsContext::full(&runtime).context("Cannot initialize custom query context")?;
    let script_error = || {
        anyhow::anyhow!("Custom query script failed: check syntax, synchronous return value and execution limits")
    };
    let request_json: String = context
        .with(|ctx| {
            ctx.eval::<(), _>(format!(
                "const query = ({});",
                settings.script.trim().trim_end_matches(';')
            ))?;
            ctx.eval("JSON.stringify(query.request)")
        })
        .map_err(|_: rquickjs::Error| script_error())?;
    anyhow::ensure!(
        request_json.len() <= 65536,
        "Custom query request is too large"
    );
    let mut request: Request = serde_json::from_str(&request_json)
        .map_err(|_| anyhow::anyhow!("Invalid custom query request configuration"))?;
    let expand = |text: &str| {
        text.replace("{{baseUrl}}", settings.base_url.trim_end_matches('/'))
            .replace("{{apiKey}}", key)
    };
    request.url = expand(&request.url);
    let target = reqwest::Url::parse(&request.url)
        .map_err(|_| anyhow::anyhow!("Invalid custom query request URL"))?;
    anyhow::ensure!(
        target.origin() == base.origin()
            && target.username().is_empty()
            && target.password().is_none()
            && target.fragment().is_none(),
        "Custom query request must use the configured base URL origin"
    );
    anyhow::ensure!(
        matches!(request.method.as_str(), "GET" | "POST"),
        "Custom query supports GET or POST"
    );
    anyhow::ensure!(request.headers.len() <= 32, "Too many custom query headers");
    let client = network.client()?;
    let mut builder = client
        .request(request.method.parse()?, target)
        .timeout(Duration::from_secs(settings.timeout_seconds));
    for (name, value) in request.headers {
        anyhow::ensure!(
            ![
                "host",
                "content-length",
                "transfer-encoding",
                "connection",
                "proxy-authorization"
            ]
            .contains(&name.to_ascii_lowercase().as_str()),
            "Unsupported custom query header"
        );
        builder = builder.header(&name, expand(&value));
    }
    if let Some(body) = request.body {
        let body = if let Some(text) = body.as_str() {
            expand(text)
        } else {
            // Substitute individual JSON strings, so quotes in credentials remain data.
            fn replace(value: &mut Value, expand: &impl Fn(&str) -> String) {
                match value {
                    Value::String(text) => *text = expand(text),
                    Value::Array(values) => values.iter_mut().for_each(|v| replace(v, expand)),
                    Value::Object(values) => values.values_mut().for_each(|v| replace(v, expand)),
                    _ => (),
                }
            }
            let mut value = body;
            replace(&mut value, &expand);
            builder = builder.header("Content-Type", "application/json");
            serde_json::to_string(&value)?
        };
        builder = builder.body(body);
    }
    let response = builder
        .send()
        .map_err(|_| anyhow::anyhow!("Custom query request failed or timed out"))?;
    anyhow::ensure!(
        response.status().is_success(),
        "Custom query returned HTTP {}",
        response.status().as_u16()
    );
    let mut bytes = Vec::new();
    response
        .take(262145)
        .read_to_end(&mut bytes)
        .map_err(|_| anyhow::anyhow!("Cannot read custom query response"))?;
    anyhow::ensure!(bytes.len() <= 262144, "Custom query response is too large");
    let raw: Value = serde_json::from_slice(&bytes)
        .map_err(|_| anyhow::anyhow!("Custom query returned invalid JSON"))?;
    let deadline = Instant::now() + Duration::from_millis(500);
    runtime.set_interrupt_handler(Some(Box::new(move || Instant::now() >= deadline)));
    let extracted: String = context
        .with(|ctx| {
            ctx.globals().set(
                "response",
                ctx.json_parse(serde_json::to_string(&raw).unwrap())?,
            )?;
            ctx.eval("JSON.stringify(query.extractor(response))")
        })
        .map_err(|_: rquickjs::Error| script_error())?;
    anyhow::ensure!(extracted.len() <= 32768, "Custom query result is too large");
    let value: Value = serde_json::from_str(&extracted)
        .map_err(|_| anyhow::anyhow!("Invalid custom query result"))?;
    let result = normalize(value)?;
    fn contains_secret(value: &Value, key: &str) -> bool {
        match value {
            Value::String(s) => s.contains(key),
            Value::Array(values) => values.iter().any(|v| contains_secret(v, key)),
            Value::Object(values) => values.values().any(|v| contains_secret(v, key)),
            _ => false,
        }
    }
    anyhow::ensure!(
        key.is_empty() || !contains_secret(&result, key),
        "Custom query result contains a credential"
    );
    Ok(result)
}

fn text(value: &Value, field: &str, max: usize) -> Result<Option<String>> {
    match value.get(field).filter(|v| !v.is_null()) {
        None => Ok(None),
        Some(v) => {
            let s = v
                .as_str()
                .context("Custom query text field must be a string")?
                .trim();
            anyhow::ensure!(
                !s.is_empty() && s.chars().count() <= max && !s.chars().any(char::is_control),
                "Invalid custom query text field"
            );
            Ok(Some(s.into()))
        }
    }
}
fn amount(value: &Value, field: &str) -> Result<Option<Decimal>> {
    value
        .get(field)
        .filter(|v| !v.is_null())
        .map(|v| {
            anyhow::ensure!(
                v.is_string() || v.is_number(),
                "Custom query amounts must be decimal strings or numbers"
            );
            Decimal::from_str(v.as_str().unwrap_or(&v.to_string()))
                .context("Invalid custom query amount")
        })
        .transpose()
}
pub(super) fn normalize(value: Value) -> Result<Value> {
    let rows = if let Some(rows) = value.as_array() {
        rows.clone()
    } else {
        vec![value]
    };
    anyhow::ensure!(
        !rows.is_empty() && rows.len() <= 20,
        "Custom query must return 1–20 items"
    );
    let mut ids = HashSet::new();
    let mut items = Vec::new();
    for row in &rows {
        anyhow::ensure!(row.is_object(), "Custom query item must be an object");
        if let Some(valid) = row.get("isValid") {
            anyhow::ensure!(
                valid.as_bool() == Some(true),
                "Custom query reports an invalid account or result"
            );
        }
        let id = text(row, "id", 80)?
            .or_else(|| (rows.len() == 1).then(|| "default".into()))
            .context("Each custom query item needs a stable id")?;
        anyhow::ensure!(ids.insert(id.clone()), "Duplicate custom query item id");
        let total = amount(row, "total")?;
        let used = amount(row, "used")?;
        let remaining = amount(row, "remaining")?
            .or_else(|| total.zip(used).and_then(|(t, u)| t.checked_sub(u)));
        let percent = amount(row, "usedPercent")?.or_else(|| {
            total
                .filter(|v| *v > Decimal::ZERO)
                .zip(used.or_else(|| total.zip(remaining).and_then(|(t, r)| t.checked_sub(r))))
                .and_then(|(t, u)| u.checked_div(t)?.checked_mul(Decimal::from(100)))
        });
        anyhow::ensure!(
            remaining.is_some() || used.is_some() || total.is_some() || percent.is_some(),
            "Custom query returned no usage values"
        );
        anyhow::ensure!(
            total.is_none_or(|v| v >= Decimal::ZERO)
                && used.is_none_or(|v| v >= Decimal::ZERO)
                && percent.is_none_or(|v| v >= Decimal::ZERO && v <= Decimal::from(10000)),
            "Invalid custom query usage range"
        );
        let reset = match row.get("resetsAt").filter(|v| !v.is_null()) {
            None => None,
            Some(v) => Some(
                v.as_i64()
                    .or_else(|| {
                        v.as_str()
                            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                            .map(|d| d.timestamp())
                    })
                    .filter(|v| *v > 0 && DateTime::from_timestamp(*v, 0).is_some())
                    .context(
                        "Custom query reset must be Unix seconds or RFC 3339 with a time zone",
                    )?,
            ),
        };
        let duration = row
            .get("durationMinutes")
            .filter(|v| !v.is_null())
            .map(|v| {
                v.as_i64()
                    .filter(|v| (1..=527040).contains(v))
                    .context("Invalid custom query window duration")
            })
            .transpose()?;
        let kind = text(row, "windowType", 16)?;
        anyhow::ensure!(
            kind.as_deref()
                .is_none_or(|k| matches!(k, "fixed" | "rolling")),
            "Invalid custom query window type"
        );
        let decimal = |v: Option<Decimal>| v.map(|v| v.normalize().to_string());
        items.push(json!({"id":id,"planName":text(row,"planName",80)?,"unit":text(row,"unit",16)?,
            "remaining":decimal(remaining),"used":decimal(used),"total":decimal(total),"usedPercent":decimal(percent),
            "resetsAt":reset,"durationMinutes":duration,"windowType":kind}));
    }
    Ok(json!({"kind":"customUsage","items":items}))
}

use chrono::DateTime;

pub(super) fn test_query(engine: &Engine, vault: &Vault, args: Value) -> Result<Value> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Draft {
        id: Option<String>,
        settings: Settings,
        api_key: Option<String>,
        #[serde(default)]
        clear_api_key: bool,
    }
    let draft: Draft = serde_json::from_value(args)?;
    let key = if let Some(key) = draft.api_key.filter(|v| !v.is_empty()) {
        key
    } else if draft.settings.script.contains("{{apiKey}}") && !draft.clear_api_key {
        let id = draft.id.context("Save or enter a query API key")?;
        let store = engine.read_lock()?;
        anyhow::ensure!(
            load(&store)?
                .iter()
                .any(|v| v.account.id == id && is_provider(&v.account.provider)),
            "Unknown custom provider"
        );
        vault.read(&id)?
    } else {
        String::new()
    };
    anyhow::ensure!(key.len() <= 4096, "Custom query API key is too long");
    let network = crate::network::load(&*engine.read_lock()?)?;
    request(&draft.settings, &key, &network)
}

pub(super) fn session_sources(engine: &Engine, args: Value) -> Result<Value> {
    let harness = args["harness"].as_str().context("Missing harness")?;
    let session = args["sessionId"].as_str().context("Missing session ID")?;
    anyhow::ensure!(
        harness.len() <= 64 && session.len() <= 512,
        "Invalid session identity"
    );
    let store = engine.read_lock()?;
    let accounts = load(&store)?;
    let mut stmt = store.conn.prepare_cached("SELECT provider,model,SUM(request_count) FROM usage_index WHERE origin='local' AND deleted=0 AND harness=?1 AND session_id=?2 GROUP BY provider,model ORDER BY provider,model")?;
    let rows = stmt
        .query_map(rusqlite::params![harness, session], |r| {
            Ok((
                r.get::<_, Option<String>>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(json!(rows.into_iter().map(|(provider,model,requests)| {
        let account = provider.as_ref().and_then(|provider|accounts.iter().find(|v|is_provider(&v.account.provider) && v.account.links.iter().any(|l|l.harness==harness && &l.provider==provider)));
        json!({"providerId":provider,"model":model,"requests":requests,"providerName":account.map(|v|&v.account.name),"accountId":account.map(|v|&v.account.id)})
    }).collect::<Vec<_>>()))
}
