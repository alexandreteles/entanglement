use std::{env, fs, path::PathBuf, process::Command};

/// Embed the grammar assets from the dependency selected by Cargo.
///
/// Return an error if Cargo metadata or a required upstream asset is absent.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = Command::new(env::var_os("CARGO").ok_or("CARGO is absent")?)
        .args(["metadata", "--offline", "--locked", "--format-version", "1"])
        .output()?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned().into());
    }
    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    let package = metadata["packages"]
        .as_array()
        .ok_or("Cargo packages are absent")?
        .iter()
        .find(|package| package["name"] == "tree-sitter-rust")
        .ok_or("The Rust grammar is absent")?;
    let manifest = PathBuf::from(
        package["manifest_path"]
            .as_str()
            .ok_or("No manifest path")?,
    );
    let root = manifest.parent().ok_or("No grammar directory")?;
    let assets = [
        "tree-sitter.json",
        "LICENSE",
        "src/parser.c",
        "src/scanner.c",
        "src/grammar.json",
        "src/tree_sitter/parser.h",
        "src/tree_sitter/alloc.h",
        "src/tree_sitter/array.h",
    ];
    let mut generated = String::from("pub const RUST_ASSETS: &[(&str, &[u8])] = &[\n");
    for name in assets {
        let path = root.join(name);
        if !path.is_file() {
            return Err(format!("Grammar asset is absent: {}", path.display()).into());
        }
        println!("cargo:rerun-if-changed={}", path.display());
        generated.push_str(&format!("({name:?}, include_bytes!({:?})),\n", path));
    }
    generated.push_str("];\n");
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").ok_or("No output directory")?)
            .join("grammar_assets.rs"),
        generated,
    )?;
    println!("cargo:rerun-if-changed=Cargo.lock");
    Ok(())
}
