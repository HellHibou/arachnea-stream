use std::{env, fs, path::PathBuf};

fn main() {
    embed_avatars();
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

/// Validates the crate-owned avatar catalogue and generates embedded byte references.
fn embed_avatars() {
    use std::collections::HashSet;
    use std::path::Component;
    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("resources/avatars");
    println!("cargo:rerun-if-changed={}", root.display());
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("avatars.json")).unwrap()).unwrap();
    assert_eq!(manifest["schema_version"], 1);
    let mut ids = HashSet::new();
    let mut generated = String::from("&[\n");
    for avatar in manifest["avatars"].as_array().expect("avatar list") {
        let id = avatar["id"].as_str().expect("avatar ID");
        assert!(
            !id.is_empty()
                && id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
            "invalid avatar ID"
        );
        assert!(ids.insert(id), "duplicate avatar ID");
        let file = std::path::Path::new(avatar["file"].as_str().expect("avatar file"));
        assert!(
            !file.as_os_str().is_empty()
                && file.components().all(|c| matches!(c, Component::Normal(_))),
            "invalid avatar path"
        );
        let path = root.join(file).canonicalize().expect("missing avatar file");
        assert!(
            path.starts_with(root.canonicalize().unwrap()),
            "avatar escapes directory"
        );
        let bytes = fs::read(&path).unwrap();
        let mime = if bytes.starts_with(b"\x89PNG\r\n\x1a\n")
            && bytes.len() >= 33
            && &bytes[12..16] == b"IHDR"
        {
            "image/png"
        } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) && bytes.ends_with(&[0xff, 0xd9]) {
            "image/jpeg"
        } else if bytes.len() >= 16 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
            "image/webp"
        } else {
            panic!("unsupported avatar image: {}", path.display())
        };
        generated.push_str(&format!(
            "({id:?}, {mime:?}, include_bytes!({:?})),\n",
            path.to_str().unwrap()
        ));
    }
    assert!(
        ids.contains(
            manifest["fallback_avatar_id"]
                .as_str()
                .expect("fallback ID")
        ),
        "missing fallback avatar"
    );
    generated.push_str("]");
    fs::write(
        PathBuf::from(env::var("OUT_DIR").unwrap()).join("avatars.rs"),
        generated,
    )
    .unwrap();
}
