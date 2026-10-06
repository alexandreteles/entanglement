mod assets;
mod go;
mod python;
pub(crate) mod query;
mod registry;
pub mod rust;
mod rust_tokens;
mod web;

pub(crate) use registry::Registry;

use std::collections::HashMap;
use std::ops::Range;

use tree_sitter::{Language, Tree};

use crate::Result;
use crate::metrics::halstead::HalsteadToken;
use crate::metrics::{SyntaxEvent, cyclomatic::FunctionScope};
use crate::model::{Definition, Export, Import, LocalBinding, Reference};

#[derive(Debug, Clone)]
pub(crate) struct InjectionRequest {
    pub language: String,
    pub range: Range<usize>,
    /// The source ranges owned by the injected language. These may leave host
    /// expressions between them, such as `${...}` in a tagged template.
    pub guest_ranges: Vec<tree_sitter::Range>,
    pub priority: i32,
    pub inherit_scope: bool,
    /// Assign injected functions, decisions, and tokens to the host metric context.
    pub inherit_metrics: bool,
    /// Reuse the host semantic context so names can cross the language boundary.
    pub inherit_context: bool,
    /// Expose top-level injected bindings to the host while keeping guest lookup isolated.
    pub share_bindings: bool,
    /// Keep exports emitted by the injected analyzer in the file-module surface.
    pub publish_exports: bool,
    /// Ignore optional language labels unless a registered analyzer matches.
    pub registered_only: bool,
}

impl InjectionRequest {
    /// Combine duplicate requests for the same language and source range.
    pub(crate) fn merge_duplicate(&mut self, other: &Self) {
        debug_assert_eq!(self.language, other.language);
        debug_assert_eq!(self.range, other.range);

        if other.priority > self.priority {
            self.inherit_scope = other.inherit_scope;
            self.inherit_metrics = other.inherit_metrics;
            self.inherit_context = other.inherit_context;
            self.share_bindings = other.share_bindings;
            self.publish_exports = other.publish_exports;
            self.registered_only = other.registered_only;
        } else if other.priority == self.priority {
            self.inherit_scope |= other.inherit_scope;
            self.inherit_metrics |= other.inherit_metrics;
            self.inherit_context |= other.inherit_context;
            self.share_bindings |= other.share_bindings;
            self.publish_exports &= other.publish_exports;
            self.registered_only &= other.registered_only;
        }
        self.priority = self.priority.max(other.priority);
        self.guest_ranges = query::ranges::intersect_sets(&self.guest_ranges, &other.guest_ranges);
    }
}

#[derive(Default)]
pub(crate) struct CapturedTree {
    pub events: Vec<SyntaxEvent>,
    pub parents: HashMap<usize, Option<usize>>,
    pub functions: Vec<FunctionScope>,
    pub tokens: Vec<HalsteadToken>,
    pub definitions: Vec<Definition>,
    pub imports: Vec<Import>,
    pub exports: Vec<Export>,
    pub references: Vec<Reference>,
    pub locals: Vec<LocalBinding>,
    pub injections: Vec<InjectionRequest>,
}

pub(crate) trait LanguageHandler: Send + Sync {
    /// Capture syntax facts, optionally collecting Halstead terminal tokens.
    fn capture(&self, tree: &Tree, source: &[u8], include_tokens: bool) -> Result<CapturedTree>;
}

/// Resolver behavior and relative-file rules declared by a language descriptor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResolutionFamily {
    RustCrates,
    FileModules,
    GoPackages,
    None,
}

/// Declarative rules for resolving relative file-module imports.
#[derive(Debug, Clone, Copy)]
pub(crate) struct FileModuleRules {
    pub extensions: &'static [&'static str],
    pub remaps: &'static [(&'static str, &'static [&'static str])],
    pub index_stems: &'static [&'static str],
    pub unresolved_prefixes: &'static [&'static str],
    pub layout: FileModuleLayout,
    pub module_bindings_shadow: bool,
    pub wildcard_imports_shadow: bool,
}

/// File layout used by a registered file-module strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FileModuleLayout {
    SlashRelative,
    Dotted {
        roots: &'static [&'static str],
        project_markers: &'static [&'static str],
        submodule_imports: bool,
    },
}

/// Register one grammar scope, analyzer constructor, and project-root convention.
pub(crate) struct LanguageSpec {
    pub scope: &'static str,
    pub resolution_family: ResolutionFamily,
    pub project_manifests: &'static [&'static str],
    pub file_module_rules: Option<&'static FileModuleRules>,
    pub build: fn(&str, &Language) -> Result<Box<dyn LanguageHandler>>,
}

#[derive(Clone)]
pub(crate) struct LanguageChoice {
    pub id: String,
    pub name: String,
    pub language: Language,
    pub resolution_family: ResolutionFamily,
    pub file_module_rules: Option<&'static FileModuleRules>,
}
