mod selection;

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, OnceLock};

use tree_sitter_loader::Loader;

use super::{LanguageHandler, LanguageSpec, assets, rust, web};
use crate::Result;

pub(crate) struct Registry {
    loader: Loader,
    handlers: HashMap<String, Box<dyn LanguageHandler>>,
    specs: HashMap<&'static str, &'static LanguageSpec>,
}

static REGISTRY: OnceLock<std::result::Result<Arc<Registry>, String>> = OnceLock::new();

impl Registry {
    /// Share one metadata loader and immutable language handlers per process.
    pub(crate) fn shared() -> Result<Arc<Self>> {
        REGISTRY
            .get_or_init(|| Self::new().map(Arc::new).map_err(|error| error.to_string()))
            .as_ref()
            .map(Arc::clone)
            .map_err(|error| std::io::Error::other(error.clone()).into())
    }

    /// Load grammar metadata and build handlers for supported language descriptors.
    fn new() -> Result<Self> {
        let mut loader = Loader::new()?;
        let grammar_root = assets::prepare(&loader)?;
        loader.parser_lib_path = grammar_root.join("lib");
        for grammar in assets::GRAMMAR_ROOTS {
            loader.find_language_configurations_at_path(&grammar_root.join(grammar), false)?;
        }
        let specs = registered_specs()?;
        let mut handlers = HashMap::new();
        let mut loaded_scopes = HashSet::new();
        for (configuration, _) in loader.get_all_language_configurations() {
            let Some(scope) = configuration.scope.as_deref() else {
                continue;
            };
            let Some(spec) = specs.get(scope) else {
                continue;
            };
            if !loaded_scopes.insert(scope.to_owned()) {
                return Err(std::io::Error::other(format!(
                    "Multiple grammar configurations use registered scope {scope}"
                ))
                .into());
            }
            let language = loader.language_for_configuration(configuration)?;
            handlers.insert(scope.to_owned(), (spec.build)(scope, &language)?);
        }
        ensure_all_specs_loaded(&specs, &handlers)?;
        Ok(Self {
            loader,
            handlers,
            specs,
        })
    }
}

fn registered_specs() -> Result<HashMap<&'static str, &'static LanguageSpec>> {
    let mut specs = HashMap::new();
    for spec in std::iter::once(&rust::LANGUAGE_SPEC).chain(web::LANGUAGE_SPECS.iter()) {
        if specs.insert(spec.scope, spec).is_some() {
            return Err(std::io::Error::other(format!(
                "Duplicate language descriptor for scope {}",
                spec.scope
            ))
            .into());
        }
    }
    Ok(specs)
}

fn ensure_all_specs_loaded(
    specs: &HashMap<&'static str, &'static LanguageSpec>,
    handlers: &HashMap<String, Box<dyn LanguageHandler>>,
) -> Result<()> {
    if let Some(missing) = specs.keys().find(|scope| !handlers.contains_key(**scope)) {
        return Err(std::io::Error::other(format!(
            "No grammar configuration was loaded for registered scope {missing}"
        ))
        .into());
    }
    Ok(())
}
