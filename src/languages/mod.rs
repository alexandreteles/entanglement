mod assets;
pub mod rust;
mod rust_tokens;

use std::collections::HashMap;
use std::ops::Range;
use std::path::Path;
use std::sync::{Arc, OnceLock};

use tree_sitter::{Language, Tree};
use tree_sitter_loader::Loader;

use crate::Result;
use crate::metrics::halstead::HalsteadToken;
use crate::metrics::{SyntaxEvent, cyclomatic::FunctionScope};
use crate::model::{Definition, Import, LocalBinding, Reference};

const RUST_SCOPE: &str = "source.rust";

#[derive(Debug, Clone)]
pub(crate) struct InjectionRequest {
    pub language: String,
    pub range: Range<usize>,
    pub child_ranges: Vec<tree_sitter::Range>,
    pub start_point: tree_sitter::Point,
    pub end_point: tree_sitter::Point,
    pub include_children: bool,
    pub priority: i32,
}

pub(crate) struct CapturedTree {
    pub events: Vec<SyntaxEvent>,
    pub parents: HashMap<usize, Option<usize>>,
    pub functions: Vec<FunctionScope>,
    pub tokens: Vec<HalsteadToken>,
    pub definitions: Vec<Definition>,
    pub imports: Vec<Import>,
    pub references: Vec<Reference>,
    pub locals: Vec<LocalBinding>,
    pub injections: Vec<InjectionRequest>,
}

pub(crate) trait LanguageHandler: Send + Sync {
    /// Capture syntax facts, optionally collecting Halstead terminal tokens.
    fn capture(&self, tree: &Tree, source: &[u8], include_tokens: bool) -> Result<CapturedTree>;
}

#[derive(Clone)]
pub(crate) struct LanguageChoice {
    pub id: String,
    pub name: String,
    pub language: Language,
}

pub(crate) struct Registry {
    loader: Loader,
    handlers: HashMap<String, Box<dyn LanguageHandler>>,
}

static REGISTRY: OnceLock<std::result::Result<Arc<Registry>, String>> = OnceLock::new();

impl Registry {
    /// Share one metadata loader and immutable language handlers per process.
    pub fn shared() -> Result<Arc<Self>> {
        REGISTRY
            .get_or_init(|| Self::new().map(Arc::new).map_err(|error| error.to_string()))
            .as_ref()
            .map(Arc::clone)
            .map_err(|error| std::io::Error::other(error.clone()).into())
    }

    /// Load grammar metadata and build handlers for supported languages.
    fn new() -> Result<Self> {
        let mut loader = Loader::new()?;
        let grammar_root = assets::prepare(&loader)?;
        loader.parser_lib_path = grammar_root.join("lib");
        for grammar in assets::GRAMMAR_ROOTS {
            loader.find_language_configurations_at_path(&grammar_root.join(grammar), false)?;
        }
        let (configuration, _) = loader
            .get_all_language_configurations()
            .into_iter()
            .find(|(configuration, _)| configuration.scope.as_deref() == Some(RUST_SCOPE))
            .ok_or_else(|| std::io::Error::other("The Rust grammar was not registered"))?;
        let id = configuration
            .scope
            .as_deref()
            .ok_or("Rust grammar has no scope")?
            .to_owned();
        let language = loader.language_for_configuration(configuration)?;
        let handler = Box::new(rust::Analyzer::new(&language)?);
        Ok(Self {
            loader,
            handlers: HashMap::from([(id, handler as Box<dyn LanguageHandler>)]),
        })
    }

    /// Select a registered language from grammar metadata for a file path.
    pub fn select_file(&self, path: &Path) -> Result<Option<LanguageChoice>> {
        let selected = self.loader.language_configuration_for_file_name(path)?;
        let selected = match selected {
            Some(selected) => Some(selected),
            None if path.is_file() => self
                .loader
                .language_configuration_for_first_line_regex(path)?,
            None => None,
        };
        let Some((language, configuration)) = selected else {
            return Ok(None);
        };
        let Some(id) = configuration.scope.clone() else {
            return Ok(None);
        };
        Ok(self.handlers.contains_key(&id).then(|| LanguageChoice {
            id,
            name: configuration.language_name.clone(),
            language,
        }))
    }

    /// Select a registered language from loader injection metadata.
    pub fn select_injection(&self, name: &str) -> Result<Option<LanguageChoice>> {
        let Some((language, configuration)) = self
            .loader
            .language_configuration_for_injection_string(name)?
        else {
            return Ok(None);
        };
        let Some(id) = configuration.scope.clone() else {
            return Ok(None);
        };
        Ok(self.handlers.contains_key(&id).then(|| LanguageChoice {
            id,
            name: configuration.language_name.clone(),
            language,
        }))
    }

    /// Run the registered language query and return normalized syntax facts.
    /// Collect Halstead terminal tokens only when `include_tokens` is true.
    pub fn capture(
        &self,
        id: &str,
        tree: &Tree,
        source: &[u8],
        include_tokens: bool,
    ) -> Result<CapturedTree> {
        self.handlers
            .get(id)
            .ok_or_else(|| -> crate::Error {
                std::io::Error::other(format!("No analyzer is registered for language {id}")).into()
            })?
            .capture(tree, source, include_tokens)
    }
}
