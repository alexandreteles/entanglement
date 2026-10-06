use std::path::Path;

use tree_sitter::Tree;

use super::Registry;
use crate::Result;
use crate::languages::{CapturedTree, LanguageChoice};

impl Registry {
    /// Select a registered language from grammar metadata for a file path.
    pub(crate) fn select_file(&self, path: &Path) -> Result<Option<LanguageChoice>> {
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
        self.choice(
            configuration.scope.clone(),
            configuration.language_name.clone(),
            language,
        )
    }

    /// Select a registered language from loader injection metadata.
    pub(crate) fn select_injection(&self, name: &str) -> Result<Option<LanguageChoice>> {
        let Some((language, configuration)) = self
            .loader
            .language_configuration_for_injection_string(name)?
        else {
            return Ok(None);
        };
        self.choice(
            configuration.scope.clone(),
            configuration.language_name.clone(),
            language,
        )
    }

    /// Run the registered language query and return normalized syntax facts.
    /// Collect Halstead terminal tokens only when `include_tokens` is true.
    pub(crate) fn capture(
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

    /// Return all manifest names declared by registered language descriptors.
    pub(crate) fn project_manifests(&self) -> Vec<&'static str> {
        let mut manifests = self
            .specs
            .values()
            .flat_map(|spec| spec.project_manifests.iter().copied())
            .collect::<Vec<_>>();
        manifests.sort_unstable();
        manifests.dedup();
        manifests
    }

    fn choice(
        &self,
        scope: Option<String>,
        name: String,
        language: tree_sitter::Language,
    ) -> Result<Option<LanguageChoice>> {
        let Some(id) = scope.filter(|id| self.handlers.contains_key(id)) else {
            return Ok(None);
        };
        let spec = self.specs[id.as_str()];
        Ok(Some(LanguageChoice {
            id,
            name,
            language,
            resolution_family: spec.resolution_family,
            file_module_rules: spec.file_module_rules,
        }))
    }
}
