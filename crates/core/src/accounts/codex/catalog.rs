//! Editable device-local allowance estimates, separate from subscription fees and prices.
use super::*;
use std::{
    collections::{BTreeMap, HashSet},
    fs,
};
const META: &str = "codex_allowance_catalog_v1";
const MAX_BYTES: usize = 262144;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Plan {
    pub id: String,
    pub name: String,
    pub monthly_fee_usd: String,
    pub theoretical_multiplier: u32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Capacity {
    pub api_equivalent_usd: String,
    pub tokens: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Samples {
    pub weekly: Option<Capacity>,
    pub rolling: Option<Capacity>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Model {
    pub id: String,
    pub name: String,
    pub aliases: Vec<String>,
    pub notes: String,
    pub estimates: BTreeMap<String, Samples>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubscriptionSpeed {
    pub source_url: String,
    pub checked_at: String,
    pub fast_multiplier: String,
    pub purchased_credits_fast_multiplier: String,
    pub fast_model_ids: Vec<String>,
}
fn default_subscription_speed() -> SubscriptionSpeed {
    let embedded: Value =
        serde_json::from_str(include_str!("../../../resources/codex-allowances.json"))
            .expect("vendored Codex allowances");
    serde_json::from_value(embedded["subscriptionSpeed"].clone())
        .expect("valid subscription speed rules")
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Catalog {
    pub schema_version: u32,
    pub version: String,
    pub source_url: String,
    pub checked_at: String,
    pub notes: String,
    pub default_mode: String,
    pub theoretical_fallback: bool,
    pub plus_weekly_to_five_hour_divisor: Option<String>,
    pub bucket_ids: Vec<String>,
    pub plans: Vec<Plan>,
    pub models: Vec<Model>,
    #[serde(default = "default_subscription_speed")]
    pub subscription_speed: SubscriptionSpeed,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Resolved {
    pub api_equivalent_usd: String,
    pub tokens: Option<String>,
    pub basis: &'static str,
    pub from_plan: String,
    pub from_window: &'static str,
    pub multiplier: String,
}
pub fn defaults() -> Catalog {
    serde_json::from_str(include_str!("../../../resources/codex-allowances.json"))
        .expect("vendored Codex allowances")
}
pub fn revision(c: &Catalog) -> String {
    crate::harness::files::hash(&serde_json::to_vec(c).expect("serializable catalog"))
}
fn number(value: &str) -> Result<Decimal> {
    anyhow::ensure!(value.len() <= 40, "Codex allowance decimal is too long");
    let n = Decimal::from_str(value).context("Invalid Codex allowance decimal")?;
    anyhow::ensure!(
        n >= Decimal::new(1, 6) && n <= Decimal::from(1_000_000_000_000_000u64),
        "Codex allowance number out of range"
    );
    Ok(n)
}
pub fn validate(c: &Catalog) -> Result<()> {
    anyhow::ensure!(
        serde_json::to_vec(c)?.len() <= MAX_BYTES,
        "Codex allowance config too large"
    );
    anyhow::ensure!(
        c.schema_version == 1 && !c.version.is_empty() && c.version.len() <= 100,
        "Invalid Codex allowance schema or version"
    );
    anyhow::ensure!(
        c.source_url.starts_with("https://") && c.source_url.len() <= 2048 && c.notes.len() <= 8000,
        "Invalid Codex allowance source"
    );
    NaiveDate::parse_from_str(&c.checked_at, "%Y-%m-%d").context("Invalid Codex allowance date")?;
    let speed = &c.subscription_speed;
    anyhow::ensure!(
        speed.source_url.starts_with("https://") && speed.source_url.len() <= 2048,
        "Invalid subscription speed source"
    );
    NaiveDate::parse_from_str(&speed.checked_at, "%Y-%m-%d")
        .context("Invalid subscription speed date")?;
    for value in [
        &speed.fast_multiplier,
        &speed.purchased_credits_fast_multiplier,
    ] {
        let value = number(value)?;
        anyhow::ensure!(
            value >= Decimal::ONE && value <= Decimal::from(100),
            "Invalid subscription speed multiplier"
        );
    }
    let mut speed_models = HashSet::new();
    anyhow::ensure!(
        speed.fast_model_ids.len() <= 256
            && speed.fast_model_ids.iter().all(|m| !m.is_empty()
                && m.len() <= 200
                && speed_models.insert(crate::pricing::normalize(m))),
        "Invalid subscription speed models"
    );
    anyhow::ensure!(
        ["modeldial", "theoretical"].contains(&c.default_mode.as_str()),
        "Invalid Codex allowance mode"
    );
    if let Some(value) = &c.plus_weekly_to_five_hour_divisor {
        let n = number(value)?;
        anyhow::ensure!(
            n >= Decimal::ONE && n <= Decimal::from(1000),
            "Invalid Plus weekly / five-hour divisor"
        );
    }
    anyhow::ensure!(
        !c.bucket_ids.is_empty()
            && c.bucket_ids.len() <= 32
            && c.bucket_ids.iter().all(|s| !s.is_empty() && s.len() <= 128),
        "Invalid Codex allowance buckets"
    );
    let mut ids = HashSet::new();
    for plan in &c.plans {
        multiplier(&plan.id)?;
        anyhow::ensure!(
            ids.insert(plan.id.as_str())
                && !plan.name.is_empty()
                && plan.name.len() <= 80
                && (1..=1000).contains(&plan.theoretical_multiplier),
            "Invalid Codex allowance plan"
        );
        number(&plan.monthly_fee_usd)?;
    }
    anyhow::ensure!(ids.len() == 4, "Configure all four Codex plans");
    anyhow::ensure!(
        !c.models.is_empty() && c.models.len() <= 256,
        "Invalid Codex allowance model count"
    );
    let mut names = HashSet::new();
    for model in &c.models {
        anyhow::ensure!(
            !model.name.is_empty()
                && model.name.len() <= 120
                && model.notes.len() <= 4000
                && model.aliases.len() <= 64,
            "Invalid Codex allowance model"
        );
        for name in std::iter::once(&model.id).chain(model.aliases.iter()) {
            let normalized = crate::pricing::normalize(name);
            anyhow::ensure!(
                !normalized.is_empty() && name.len() <= 200 && names.insert(normalized),
                "Duplicate or invalid Codex allowance model alias"
            );
        }
        for (id, samples) in &model.estimates {
            multiplier(id)?;
            anyhow::ensure!(
                id == "plus" || samples.rolling.is_none(),
                "Pro plans only have weekly reference allowances"
            );
            for sample in [&samples.weekly, &samples.rolling].into_iter().flatten() {
                number(&sample.api_equivalent_usd)?;
                if let Some(tokens) = &sample.tokens {
                    number(tokens)?;
                }
            }
        }
    }
    Ok(())
}
pub fn load(s: &Store) -> Result<Catalog> {
    let c = s.meta(META)?.unwrap_or_else(defaults);
    validate(&c)?;
    Ok(c)
}
pub fn find<'a>(c: &'a Catalog, raw: &str) -> Option<&'a Model> {
    let key = crate::pricing::normalize(raw);
    c.models.iter().find(|m| {
        std::iter::once(&m.id)
            .chain(m.aliases.iter())
            .any(|v| crate::pricing::normalize(v) == key)
    })
}
pub fn plan<'a>(c: &'a Catalog, id: &str) -> Result<&'a Plan> {
    c.plans
        .iter()
        .find(|p| p.id == id)
        .context("Codex allowance plan is not configured")
}
pub fn resolve(
    c: &Catalog,
    model: &Model,
    plan_id: &str,
    window: &str,
    mode: &str,
) -> Result<Option<Resolved>> {
    anyhow::ensure!(
        ["modeldial", "theoretical"].contains(&mode),
        "Invalid Codex allowance mode"
    );
    let selected = plan(c, plan_id)?;
    let plus = plan(c, "plus")?;
    if window != "weekly" && window != "rolling" || window == "rolling" && plan_id != "plus" {
        return Ok(None);
    }
    let sample = |id: &str| {
        model.estimates.get(id).and_then(|s| {
            if window == "rolling" {
                s.rolling.as_ref()
            } else {
                s.weekly.as_ref()
            }
        })
    };
    let mut base = sample(plan_id);
    let mut basis = "modeldial";
    let mut from_plan = plan_id;
    let mut from_window = window;
    let mut factor = Decimal::ONE;
    if mode == "theoretical" || base.is_none() && (c.theoretical_fallback || plan_id == "plus") {
        base = sample("plus");
        from_plan = "plus";
        factor = Decimal::from(selected.theoretical_multiplier)
            / Decimal::from(plus.theoretical_multiplier);
        basis = if plan_id == "plus" {
            "modeldial"
        } else {
            "theoretical"
        };
        if base.is_none() && window == "rolling" {
            if let Some(divisor) = &c.plus_weekly_to_five_hour_divisor {
                base = model.estimates.get("plus").and_then(|s| s.weekly.as_ref());
                factor /= number(divisor)?;
                basis = "weekly_division";
                from_window = "weekly";
            }
        }
    }
    base.map(|b| -> Result<_> {
        let scale = |v: &str| -> Result<String> {
            Ok(number(v)?
                .checked_mul(factor)
                .context("Codex allowance estimate precision exceeded")?
                .round_dp(18)
                .normalize()
                .to_string())
        };
        Ok(Resolved {
            api_equivalent_usd: scale(&b.api_equivalent_usd)?,
            tokens: b.tokens.as_deref().map(scale).transpose()?,
            basis,
            from_plan: from_plan.into(),
            from_window: if from_window == "weekly" {
                "weekly"
            } else {
                "rolling"
            },
            multiplier: factor.round_dp(18).normalize().to_string(),
        })
    })
    .transpose()
}
fn read_file(path: &Path) -> Result<Option<Vec<u8>>> {
    let file = match fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let mut bytes = Vec::new();
    file.take(MAX_BYTES as u64 + 1).read_to_end(&mut bytes)?;
    anyhow::ensure!(bytes.len() <= MAX_BYTES, "Codex allowance file too large");
    Ok(Some(bytes))
}
fn file_info(path: &Path) -> Value {
    match read_file(path) {
        Ok(Some(bytes)) => {
            let hash = crate::harness::files::hash(&bytes);
            let parsed = serde_json::from_slice::<Catalog>(&bytes)
                .ok()
                .filter(|c| validate(c).is_ok());
            json!({"revision":hash,"catalog":parsed,"error":if parsed.is_none(){Some("Local allowance file is invalid; active estimates were preserved")}else{None}})
        }
        Ok(None) => json!({"revision":null,"catalog":null,"error":null}),
        Err(_) => {
            json!({"revision":null,"catalog":null,"error":"Cannot read local Codex allowance file"})
        }
    }
}
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let temp = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| -> Result<()> {
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        fs::rename(&temp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result.context("Cannot save Codex allowance file")
}
pub fn info(s: &Store, path: &Path) -> Result<Value> {
    let c = load(s)?;
    let mut tables = BTreeMap::new();
    for mode in ["modeldial", "theoretical"] {
        let mut rows = Vec::new();
        for m in &c.models {
            let mut plans = BTreeMap::new();
            for p in &c.plans {
                plans.insert(&p.id, resolve(&c, m, &p.id, "weekly", mode)?);
            }
            rows.push(json!({"id":m.id,"name":m.name,"notes":m.notes,"plans":plans}));
        }
        tables.insert(mode, rows);
    }
    Ok(
        json!({"catalog":c,"revision":revision(&c),"path":path,"file":file_info(path),"resolved":tables}),
    )
}
pub fn save(s: &Store, path: &Path, args: Value) -> Result<Value> {
    let c: Catalog = serde_json::from_value(args["catalog"].clone())
        .context("Invalid Codex allowance config JSON")?;
    validate(&c)?;
    anyhow::ensure!(
        args["expectedRevision"].as_str() == Some(revision(&load(s)?).as_str()),
        "Codex allowance rules changed; reload before saving"
    );
    let previous = read_file(path)?;
    let previous_hash = previous.as_ref().map(|b| crate::harness::files::hash(b));
    anyhow::ensure!(
        args["expectedFileRevision"].as_str() == previous_hash.as_deref(),
        "Codex allowance file changed; reload before saving"
    );
    write_atomic(path, &serde_json::to_vec_pretty(&c)?)?;
    if let Err(error) = s.set_meta(META, &c) {
        if let Some(bytes) = previous {
            write_atomic(path, &bytes)?;
        } else {
            fs::remove_file(path)?;
        }
        return Err(error);
    }
    info(s, path)
}

#[cfg(test)]
mod tests;
