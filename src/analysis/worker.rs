use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tree_sitter::{InputEdit, Parser, Range as TsRange, Tree};

use crate::Result;
use crate::input;
use crate::languages::{LanguageChoice, Registry};
use crate::metrics::selection::Selection;
use crate::model::{FileFacts, ModulePath};

use super::{
    facts,
    injections::{self, ParsedInjection},
};

/// Keep parser state for one file task and reuse it for later files.
pub(crate) struct Worker {
    pub(super) registry: Arc<Registry>,
    pub(super) selection: Selection,
    parsers: HashMap<String, Parser>,
}

/// Keep a parsed file so a later patch can reuse its syntax trees.
pub(crate) struct ParsedFile {
    pub language: String,
    pub tree: Tree,
    pub(super) injections: Vec<ParsedInjection>,
    pub facts: FileFacts,
}

impl Worker {
    /// Create a worker with the registered grammars and empty parser state.
    pub fn new(selection: Selection) -> Result<Self> {
        Ok(Self {
            registry: Registry::shared()?,
            selection,
            parsers: HashMap::new(),
        })
    }

    /// Select a registered grammar from Tree-sitter metadata.
    pub(crate) fn select_file(&self, path: &Path) -> Result<Option<LanguageChoice>> {
        self.registry.select_file(path)
    }

    /// Parse with a language choice already selected from grammar metadata.
    pub(crate) fn analyze_selected_source(
        &mut self,
        path: &Path,
        source: Vec<u8>,
        previous: Option<&ParsedFile>,
        edits: &[InputEdit],
        choice: LanguageChoice,
    ) -> Result<ParsedFile> {
        let target = input::path_identity(path)?;
        let aliases = previous
            .filter(|file| file.facts.target == target)
            .map_or_else(
                || vec![path.to_path_buf()],
                |file| file.facts.aliases.clone(),
            );
        let reusable = previous.filter(|file| {
            file.language == choice.id
                && (!edits.is_empty() || file.facts.source.as_ref() == source)
        });
        // A normalization decision can change bytes outside the user's edits.
        // Such trees must be parsed afresh on both adding and removing a prefix.
        let reusable = if let Some(previous) = reusable {
            let old = self
                .registry
                .parser_source(&choice.id, &previous.facts.source)?;
            let new = self.registry.parser_source(&choice.id, &source)?;
            (matches!(old, std::borrow::Cow::Borrowed(_))
                && matches!(new, std::borrow::Cow::Borrowed(_)))
            .then_some(previous)
        } else {
            None
        };
        let mut old_trees = BTreeMap::new();
        if let Some(previous) = reusable {
            injections::collect_old_trees(&previous.injections, edits, &mut old_trees);
        }
        let old_root = reusable.map(|file| injections::edit_tree(file.tree.clone(), edits));
        let tree = self.parse_tree(&choice, &source, &[], old_root.as_ref())?;
        let mut active = HashSet::from([(choice.id.clone(), 0, source.len())]);
        let mut next_node_id = 0;
        let mut next_context_id = 0;
        let (injections, summaries) = self.analyze_layer(
            &choice,
            &tree,
            &source,
            0..source.len(),
            &ModulePath::default(),
            None,
            None,
            choice.id.clone(),
            &mut active,
            &mut next_node_id,
            &mut next_context_id,
            &mut old_trees,
            0,
        )?;
        let mut facts = facts::build_facts(
            path,
            source.into(),
            &choice,
            summaries,
            &injections,
            self.selection,
        );
        facts.target = target;
        facts.aliases = aliases;
        Ok(ParsedFile {
            language: choice.id,
            tree,
            injections,
            facts,
        })
    }

    pub(super) fn parse_tree(
        &mut self,
        choice: &LanguageChoice,
        source: &[u8],
        included_ranges: &[TsRange],
        old_tree: Option<&Tree>,
    ) -> Result<Tree> {
        let parser_source = if included_ranges.is_empty() {
            self.registry.parser_source(&choice.id, source)?
        } else {
            let mut prepared = None;
            for range in included_ranges {
                let original = &source[range.start_byte..range.end_byte];
                if let std::borrow::Cow::Owned(normalized) =
                    self.registry.parser_source(&choice.id, original)?
                {
                    let bytes = prepared.get_or_insert_with(|| source.to_vec());
                    bytes[range.start_byte..range.end_byte].copy_from_slice(&normalized);
                }
            }
            prepared.map_or(std::borrow::Cow::Borrowed(source), std::borrow::Cow::Owned)
        };
        let old_tree = old_tree.filter(|_| self.registry.incremental_safe(&choice.id));
        let parser = self.parsers.entry(choice.id.clone()).or_default();
        parser.set_language(&choice.language)?;
        parser.set_included_ranges(included_ranges)?;
        parser
            .parse(parser_source.as_ref(), old_tree)
            .ok_or_else(|| format!("Tree-sitter canceled parsing {}", choice.name).into())
    }
}

impl Worker {
    pub(crate) fn analyze_path(&mut self, path: &Path) -> Result<Option<ParsedFile>> {
        let Some(choice) = self.select_file(path)? else {
            return Ok(None);
        };
        let target = input::path_identity(path)?;
        let source = fs::read(&target)?;
        self.analyze_selected_source(path, source, None, &[], choice)
            .map(Some)
    }

    pub(super) fn analyze_target(
        &mut self,
        target: PathBuf,
        aliases: Vec<PathBuf>,
    ) -> Result<Option<ParsedFile>> {
        let selected = aliases
            .iter()
            .find_map(|path| {
                self.select_file(path)
                    .transpose()
                    .map(|choice| choice.map(|choice| (path, choice)))
            })
            .transpose()?;
        let Some((path, choice)) = selected else {
            return Ok(None);
        };
        let source = fs::read(&target)?;
        let mut parsed = self.analyze_selected_source(path, source, None, &[], choice)?;
        parsed.facts.target = target;
        parsed.facts.aliases = aliases;
        Ok(Some(parsed))
    }
}
