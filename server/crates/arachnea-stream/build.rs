use std::{env, fs, path::PathBuf};

fn main() {
    let config_path = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("missing manifest dir"))
        .join("tauri.conf.json");
    println!("cargo:rerun-if-changed={}", config_path.display());

    let config = fs::read_to_string(&config_path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", config_path.display()));
    let config = serde_json::from_str::<serde_json::Value>(&config).unwrap_or_else(|error| {
        panic!(
            "failed to parse Tauri configuration {}: {error}",
            config_path.display()
        )
    });
    let identifier = config
        .get("identifier")
        .and_then(serde_json::Value::as_str)
        .filter(|identifier| !identifier.is_empty())
        .unwrap_or_else(|| {
            panic!(
                "Tauri configuration {} must define a non-empty identifier",
                config_path.display()
            )
        });
    println!("cargo:rustc-env=ARACHNEA_TAURI_IDENTIFIER={identifier}");

    tauri_build::build()
}
