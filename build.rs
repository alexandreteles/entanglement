use std::{
    collections::{BTreeSet, HashSet},
    env, fs,
    path::{Component, Path, PathBuf},
    process::Command,
};
use tree_sitter_loader::{Grammar, PathsJSON, TreeSitterJSON};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = Command::new(env::var_os("CARGO").ok_or("CARGO is absent")?)
        .args(["metadata", "--offline", "--locked", "--format-version", "1"])
        .output()?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned().into());
    }
    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    let mut generated = String::from("pub const GRAMMAR_ROOTS: &[&str] = &[\n");
    let mut assets = String::from("pub const GRAMMAR_ASSETS: &[(&str, &str, &[u8])] = &[\n");
    let mut roots = HashSet::new();
    for package in metadata["packages"]
        .as_array()
        .ok_or("Cargo packages are absent")?
    {
        let manifest = PathBuf::from(
            package["manifest_path"]
                .as_str()
                .ok_or("No manifest path")?,
        );
        let package_root = manifest.parent().ok_or("No grammar directory")?;
        if !package_root.join("tree-sitter.json").is_file() {
            continue;
        }
        let root = format!(
            "grammars/{}-{}",
            package["name"].as_str().ok_or("No package name")?,
            package["version"].as_str().ok_or("No package version")?
        );
        if !roots.insert(root.clone()) {
            return Err(format!("Duplicate grammar package path: {root}").into());
        }
        let metadata = TreeSitterJSON::from_file(package_root)?;
        let mut files = BTreeSet::new();
        collect(package_root, Path::new("tree-sitter.json"), &mut files)?;
        for grammar in metadata.grammars {
            add_grammar_assets(package_root, grammar, &mut files)?;
        }
        for entry in fs::read_dir(package_root)? {
            let entry = entry?;
            if entry.file_name().to_string_lossy().starts_with("LICENSE") {
                collect(
                    package_root,
                    entry.path().strip_prefix(package_root)?,
                    &mut files,
                )?;
            }
        }
        generated.push_str(&format!("{root:?},\n"));
        for file in files {
            let path = package_root.join(&file);
            assets.push_str(&format!(
                "({root:?}, {file:?}, include_bytes!({path:?})),\n"
            ));
        }
    }
    generated.push_str("];\n");
    assets.push_str("];\n");
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").ok_or("No output directory")?)
            .join("grammar_assets.rs"),
        format!("{generated}{assets}"),
    )?;
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=Cargo.lock");
    Ok(())
}

fn add_grammar_assets(
    root: &Path,
    grammar: Grammar,
    files: &mut BTreeSet<PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    collect(root, &grammar.path.unwrap_or_default().join("src"), files)?;
    for path in [
        grammar.external_files,
        grammar.highlights,
        grammar.injections,
        grammar.locals,
        grammar.tags,
    ]
    .into_iter()
    .flat_map(|paths| match paths {
        PathsJSON::Empty => vec![],
        PathsJSON::Single(path) => vec![path],
        PathsJSON::Multiple(paths) => paths,
    }) {
        collect(root, &path, files)?;
    }
    Ok(())
}

fn collect(
    root: &Path,
    path: &Path,
    files: &mut BTreeSet<PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    if path
        .components()
        .any(|component| !matches!(component, Component::Normal(_) | Component::CurDir))
    {
        return Err(format!("Grammar path escapes its package: {}", path.display()).into());
    }
    let path = root.join(path);
    println!("cargo:rerun-if-changed={}", path.display());
    if path.is_dir() {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() || entry.file_type()?.is_file() {
                collect(root, entry.path().strip_prefix(root)?, files)?;
            }
        }
    } else if path.is_file() {
        files.insert(path.strip_prefix(root)?.to_path_buf());
    } else {
        return Err(format!("Grammar asset is absent: {}", path.display()).into());
    }
    Ok(())
}
