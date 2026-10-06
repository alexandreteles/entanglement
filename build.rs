use cargo_metadata::MetadataCommand;
use std::{
    collections::{BTreeSet, HashSet},
    env, fs,
    path::{Component, Path, PathBuf},
};
use tree_sitter_loader::{Grammar, PathsJSON, TreeSitterJSON};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let metadata = MetadataCommand::new()
        .cargo_path(env::var_os("CARGO").ok_or("CARGO is absent")?)
        .other_options(vec!["--offline".into(), "--locked".into()])
        .exec()?;
    let mut generated = String::from("pub const GRAMMAR_ROOTS: &[&str] = &[\n");
    let mut assets = String::from("pub const GRAMMAR_ASSETS: &[(&str, &str, &[u8])] = &[\n");
    let mut roots = HashSet::new();
    for package in metadata.packages {
        let manifest = package.manifest_path.into_std_path_buf();
        let package_root = manifest.parent().ok_or("No grammar directory")?;
        let metadata_file = package_root.join("tree-sitter.json");
        let override_file = Path::new("grammar-metadata")
            .join(package.name.as_str())
            .join("tree-sitter.json");
        let metadata_file = if override_file.is_file() {
            println!("cargo:rerun-if-changed={}", override_file.display());
            override_file.canonicalize()?
        } else if metadata_file.is_file() {
            metadata_file
        } else {
            continue;
        };
        let root = format!("grammars/{}-{}", package.name, package.version);
        if !roots.insert(root.clone()) {
            return Err(format!("Duplicate grammar package path: {root}").into());
        }
        let metadata_directory = metadata_file.parent().ok_or("No metadata directory")?;
        let metadata = TreeSitterJSON::from_file(metadata_directory)?;
        let mut files = BTreeSet::new();
        if metadata_file.starts_with(package_root) {
            collect(package_root, Path::new("tree-sitter.json"), &mut files)?;
        } else {
            files.insert(PathBuf::from("tree-sitter.json"));
        }
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
            let path = if file == Path::new("tree-sitter.json")
                && !metadata_file.starts_with(package_root)
            {
                metadata_file.clone()
            } else {
                package_root.join(&file)
            };
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
    println!("cargo:rerun-if-changed=grammar-metadata");
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
