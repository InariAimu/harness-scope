//! Device-local presentation assets, independent of usage and account revisions.
use crate::{store::Store, Engine};
use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, io::Cursor, sync::OnceLock};

const META: &str = "icon_library_v1";
const MAX_BYTES: usize = 1024 * 1024;

#[derive(Deserialize)]
struct Preset {
    id: String,
}

fn presets() -> &'static [Preset] {
    static PRESETS: OnceLock<Vec<Preset>> = OnceLock::new();
    PRESETS.get_or_init(|| {
        serde_json::from_str(include_str!("../resources/icon-presets.json"))
            .expect("bundled icon presets")
    })
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Asset {
    id: String,
    name: String,
    data_url: String,
    width: u32,
    height: u32,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
struct Library {
    revision: u64,
    assets: BTreeMap<String, Asset>,
    harnesses: BTreeMap<String, String>,
    accounts: BTreeMap<String, String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Target {
    kind: String,
    id: String,
}

#[derive(Deserialize)]
struct SvgPolicy {
    elements: Vec<String>,
    attributes: Vec<String>,
}

fn policy() -> &'static SvgPolicy {
    static POLICY: OnceLock<SvgPolicy> = OnceLock::new();
    POLICY.get_or_init(|| {
        serde_json::from_str(include_str!("../resources/icon-svg-policy.json"))
            .expect("bundled SVG policy")
    })
}

fn valid_value(value: &str) -> bool {
    if value.contains(['\\', '@', '<', '>']) || value.chars().any(char::is_control) {
        return false;
    }
    // Only local paint/clip/mask references; no CSS escapes or external URLs.
    if value
        .to_ascii_lowercase()
        .split("url")
        .skip(1)
        .any(|part| part.trim_start().starts_with('('))
    {
        return value
            .strip_prefix("url(#")
            .and_then(|v| v.strip_suffix(')'))
            .is_some_and(|id| {
                !id.is_empty()
                    && id
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || b"_.:-".contains(&c))
            });
    }
    true
}

fn validate_svg(bytes: &[u8]) -> Result<(u32, u32)> {
    let text = std::str::from_utf8(bytes).context("Invalid SVG encoding")?;
    let doc = roxmltree::Document::parse(text).context("Invalid SVG")?;
    let root = doc.root_element();
    anyhow::ensure!(
        root.has_tag_name(("http://www.w3.org/2000/svg", "svg")),
        "Invalid SVG root"
    );
    anyhow::ensure!(doc.descendants().count() <= 4096, "SVG is too complex");
    for node in doc.descendants() {
        anyhow::ensure!(
            !node.is_pi(),
            "SVG processing instructions are not supported"
        );
        if !node.is_element() {
            continue;
        }
        anyhow::ensure!(
            node.tag_name().namespace() == Some("http://www.w3.org/2000/svg")
                && policy()
                    .elements
                    .iter()
                    .any(|v| v == node.tag_name().name()),
            "Unsupported SVG element"
        );
        for attr in node.attributes() {
            anyhow::ensure!(
                attr.namespace().is_none()
                    && policy().attributes.iter().any(|v| v == attr.name())
                    && valid_value(attr.value().trim()),
                "Unsupported SVG attribute"
            );
        }
    }
    let width: u32 = root
        .attribute("width")
        .context("Missing SVG width")?
        .parse()?;
    let height: u32 = root
        .attribute("height")
        .context("Missing SVG height")?
        .parse()?;
    Ok((width, height))
}

fn validate_asset(data_url: &str) -> Result<(String, u32, u32)> {
    anyhow::ensure!(
        data_url.len() <= MAX_BYTES * 4 / 3 + 128,
        "Icon exceeds 1 MB"
    );
    let (mime, encoded) = if let Some(encoded) = data_url.strip_prefix("data:image/png;base64,") {
        ("image/png", encoded)
    } else if let Some(encoded) = data_url.strip_prefix("data:image/svg+xml;base64,") {
        ("image/svg+xml", encoded)
    } else {
        anyhow::bail!("Icon must be a normalized PNG or SVG");
    };
    let bytes = STANDARD.decode(encoded).context("Invalid icon encoding")?;
    anyhow::ensure!(bytes.len() <= MAX_BYTES, "Icon exceeds 1 MB");
    let (width, height) = if mime == "image/png" {
        let mut decoder = png::Decoder::new(Cursor::new(&bytes));
        decoder.set_limits(png::Limits {
            bytes: 2 * 1024 * 1024,
        });
        let mut reader = decoder.read_info().context("Invalid PNG")?;
        let (w, h) = (reader.info().width, reader.info().height);
        anyhow::ensure!(
            (1..=128).contains(&w) && (1..=128).contains(&h),
            "Icon must be at most 128 × 128"
        );
        anyhow::ensure!(
            reader.info().animation_control.is_none(),
            "Animated icons are not supported"
        );
        let mut output = vec![0; reader.output_buffer_size()];
        reader
            .next_frame(&mut output)
            .context("Invalid PNG pixels")?;
        (w, h)
    } else {
        validate_svg(&bytes)?
    };
    anyhow::ensure!(
        (1..=128).contains(&width) && (1..=128).contains(&height),
        "Icon must be at most 128 × 128"
    );
    let digest = format!("{:x}", Sha256::digest(&bytes));
    Ok((digest, width, height))
}

fn load(store: &Store) -> Result<Library> {
    Ok(store.meta(META)?.unwrap_or_default())
}

fn save(store: &Store, mut library: Library) -> Result<Value> {
    library.revision += 1;
    store.set_meta(META, &library)?;
    Ok(json!(library))
}

pub(crate) fn remove_account(store: &Store, id: &str) -> Result<()> {
    let mut library = load(store)?;
    if library.accounts.remove(id).is_some() {
        save(store, library)?;
    }
    Ok(())
}

fn validate_target(engine: &Engine, store: &Store, target: &Target) -> Result<()> {
    match target.kind.as_str() {
        "harness" => {
            engine.registry.get(&target.id)?;
        }
        "account" => anyhow::ensure!(
            crate::accounts::exists(store, &target.id)?,
            "Provider account no longer exists"
        ),
        _ => anyhow::bail!("Invalid icon target"),
    }
    Ok(())
}

fn bind(library: &mut Library, target: Target, icon: Option<String>) -> Result<()> {
    if let Some(icon) = &icon {
        let valid = icon
            .strip_prefix("builtin:")
            .is_some_and(|id| presets().iter().any(|preset| preset.id == id))
            || icon
                .strip_prefix("asset:")
                .is_some_and(|id| library.assets.contains_key(id));
        anyhow::ensure!(valid, "Icon no longer exists");
    }
    let bindings = if target.kind == "harness" {
        &mut library.harnesses
    } else {
        &mut library.accounts
    };
    if let Some(icon) = icon {
        bindings.insert(target.id, icon);
    } else {
        bindings.remove(&target.id);
    }
    Ok(())
}

pub(crate) fn execute(engine: &Engine, name: &str, args: Value) -> Result<Value> {
    let store = engine.lock()?;
    let mut library = load(&store)?;
    match name {
        "icon_library" => return Ok(json!(library)),
        "set_entity_icon" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Input {
                target: Target,
                icon: Option<String>,
            }
            let input: Input = serde_json::from_value(args)?;
            validate_target(engine, &store, &input.target)?;
            bind(&mut library, input.target, input.icon)?;
        }
        "import_entity_icon" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct Input {
                target: Target,
                name: String,
                data_url: String,
            }
            let input: Input = serde_json::from_value(args)?;
            validate_target(engine, &store, &input.target)?;
            let name = input.name.trim();
            anyhow::ensure!(
                !name.is_empty()
                    && name.chars().count() <= 120
                    && !name.chars().any(char::is_control),
                "Invalid icon name"
            );
            let (id, width, height) = validate_asset(&input.data_url)?;
            if !library.assets.contains_key(&id) {
                anyhow::ensure!(
                    library.assets.len() < 64,
                    "Icon library is full; remove unused icons"
                );
                anyhow::ensure!(
                    library
                        .assets
                        .values()
                        .map(|a| a.data_url.len())
                        .sum::<usize>()
                        + input.data_url.len()
                        <= 12 * 1024 * 1024,
                    "Icon library is full; remove unused icons"
                );
                library.assets.insert(
                    id.clone(),
                    Asset {
                        id: id.clone(),
                        name: name.to_owned(),
                        data_url: input.data_url,
                        width,
                        height,
                    },
                );
            }
            // Asset and binding commit together; a failed write cannot leave an orphan upload.
            bind(&mut library, input.target, Some(format!("asset:{id}")))?;
        }
        "delete_icon_asset" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Input {
                id: String,
            }
            let input: Input = serde_json::from_value(args)?;
            let reference = format!("asset:{}", input.id);
            anyhow::ensure!(
                !library
                    .harnesses
                    .values()
                    .chain(library.accounts.values())
                    .any(|id| id == &reference),
                "Icon is still in use"
            );
            library.assets.remove(&input.id);
        }
        _ => anyhow::bail!("Unknown icon command"),
    }
    save(&store, library)
}

#[cfg(test)]
mod tests;
