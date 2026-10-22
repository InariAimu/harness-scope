use std::{env, fs, path::Path, process::Command};

fn git(root: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}
fn main() {
    let manifest = env::var("CARGO_MANIFEST_DIR").unwrap();
    let root = Path::new(&manifest).join("../..");
    for path in [
        "Cargo.toml",
        "Cargo.lock",
        "package.json",
        "package-lock.json",
        "src",
        "src-tauri",
        "crates",
        "server",
        "scripts",
        "vite.config.ts",
    ] {
        println!("cargo:rerun-if-changed={}", root.join(path).display());
    }
    for path in ["HEAD", "index", "packed-refs", "refs"] {
        if let Some(path) = git(&root, &["rev-parse", "--git-path", path]) {
            let path = root.join(path);
            if path.exists() {
                println!("cargo:rerun-if-changed={}", path.display());
            }
        }
    }
    let version = env::var("CARGO_PKG_VERSION").unwrap();
    for file in [
        "package.json",
        "package-lock.json",
        "src-tauri/tauri.conf.json",
    ] {
        let value: serde_json::Value =
            serde_json::from_slice(&fs::read(root.join(file)).unwrap()).unwrap();
        assert_eq!(
            value["version"].as_str(),
            Some(version.as_str()),
            "Version mismatch in {file}; run npm run version:sync"
        );
        if file == "package-lock.json" {
            assert_eq!(
                value["packages"][""]["version"].as_str(),
                Some(version.as_str())
            );
        }
    }
    let commit = git(&root, &["rev-parse", "HEAD"]);
    let dirty = git(
        &root,
        &["status", "--porcelain", "--untracked-files=normal"],
    )
    .map(|s| !s.is_empty());
    let tag = git(
        &root,
        &[
            "tag",
            "--points-at",
            "HEAD",
            "--list",
            &format!("v{version}"),
        ],
    );
    let release = dirty == Some(false) && tag.as_deref() == Some(format!("v{version}").as_str());
    let now = chrono::Utc::now();
    let build_id = now.format("%Y%m%d%H%M%S%3f").to_string();
    let info = serde_json::json!({
        "version": version,
        "displayVersion": if release { version.clone() } else { format!("{version}-dev.{build_id}") },
        "channel": if release { "release" } else { "dev" },
        "buildId":build_id, "builtAt":now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        "gitCommit":commit, "dirty":dirty, "profile":env::var("PROFILE").unwrap(),
        "target":env::var("TARGET").unwrap(),
    });
    fs::write(
        Path::new(&env::var("OUT_DIR").unwrap()).join("build-info.json"),
        info.to_string(),
    )
    .unwrap();
}
