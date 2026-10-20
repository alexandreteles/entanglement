//! Build corrected grammars from pinned upstream source packages.

use cargo_metadata::MetadataCommand;
use serde_json::Value;
use std::{
    env,
    error::Error,
    fs,
    path::{Path, PathBuf},
};
use tree_sitter_generate::{OptLevel, generate_parser_in_directory};

pub struct GrammarBuild {
    pub source: PathBuf,
    pub root: PathBuf,
    compiler: cc::Build,
}

impl GrammarBuild {
    /// Find the pinned source package and copy its license, metadata, and queries.
    /// Return an error if Cargo metadata, source assets, or output paths are absent.
    pub fn new(name: &str) -> Result<Self, Box<dyn Error>> {
        let metadata = MetadataCommand::new()
            .manifest_path(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml"))
            .cargo_path(env::var_os("CARGO").ok_or("CARGO is absent")?)
            .other_options(vec!["--locked".into()])
            .exec()?;
        let package = metadata
            .packages
            .iter()
            .find(|package| package.name.as_str() == name && package.source.is_some())
            .ok_or_else(|| format!("The pinned grammar is absent: {name}"))?;
        let source = package
            .manifest_path
            .parent()
            .ok_or("No grammar source directory")?
            .as_std_path()
            .to_owned();
        let root =
            PathBuf::from(env::var_os("OUT_DIR").ok_or("No output directory")?).join("grammar");
        fs::create_dir_all(&root)?;
        fs::copy(
            source.join("tree-sitter.json"),
            root.join("tree-sitter.json"),
        )?;
        for entry in fs::read_dir(&source)? {
            let entry = entry?;
            if entry.file_name().to_string_lossy().starts_with("LICENSE") {
                fs::copy(entry.path(), root.join(entry.file_name()))?;
            }
        }
        if source.join("queries").is_dir() {
            fs::create_dir_all(root.join("queries"))?;
            for entry in fs::read_dir(source.join("queries"))? {
                let entry = entry?;
                if entry.file_type()?.is_file() {
                    fs::copy(entry.path(), root.join("queries").join(entry.file_name()))?;
                }
            }
        }
        let mut compiler = cc::Build::new();
        compiler.std("c11");
        Ok(Self {
            source,
            root,
            compiler,
        })
    }

    /// Correct and generate one dialect, then add its parser and optional scanner.
    /// An empty dialect selects the package root. Grammar and I/O errors are returned.
    pub fn parser(
        &mut self,
        dialect: &str,
        correct: fn(&mut Value) -> Result<(), Box<dyn Error>>,
    ) -> Result<(), Box<dyn Error>> {
        let directory = self.root.join(dialect);
        let src = directory.join("src");
        let upstream = self.source.join(dialect).join("src");
        fs::create_dir_all(&src)?;
        let mut grammar: Value = serde_json::from_slice(&fs::read(upstream.join("grammar.json"))?)?;
        correct(&mut grammar)?;
        let input = src.join("grammar.json");
        fs::write(&input, serde_json::to_vec_pretty(&grammar)?)?;
        generate_parser_in_directory(
            &directory,
            None::<&Path>,
            Some(&input),
            15,
            None,
            None,
            true,
            OptLevel::default(),
            &mut Vec::new(),
        )?;
        self.compiler.include(&src).file(src.join("parser.c"));
        if upstream.join("scanner.c").is_file() {
            fs::copy(upstream.join("scanner.c"), src.join("scanner.c"))?;
            self.compiler.file(src.join("scanner.c"));
        }
        Ok(())
    }

    /// Compile the selected parsers and expose their asset root to the parent build.
    pub fn finish(self, library: &str) {
        self.compiler.compile(library);
        println!("cargo:root={}", self.root.display());
        println!("cargo:rerun-if-changed=build.rs");
        println!("cargo:rerun-if-changed=../build_support.rs");
    }
}
