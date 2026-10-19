//! Compiler-free extraction of static Gradle project and source-set facts.
//! Kotlin source analysis does not require these optional build files.

mod dependencies;
mod settings;
mod source_sets;

use dependencies::project_dependencies;
use settings::{includes, project_directories};
use source_sets::source_edges;

mod lexer;
mod syntax;

use lexer::{
    ident, matching, split_arguments, statement_end, statement_start, symbol_at, token_identifier,
};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use syntax::{GradleBlock, StaticScript, dependency_scope, valid_source_set_name};

#[cfg(test)]
#[path = "../../../tests/unit/kotlin_build.rs"]
mod tests;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(in crate::resolver::kotlin) enum DependencyScope {
    /// Standard production configurations such as `implementation` and `api`.
    Production,
    /// Test configurations retain the exact source-set prefix.
    Test(String),
    /// A named variant or source set such as `debug` or `commonMain`.
    Variant(String),
    /// A known but unsupported configuration that cannot provide visibility.
    Other,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(in crate::resolver::kotlin) struct ProjectDependency {
    pub path: String,
    pub configuration: String,
    pub scope: DependencyScope,
    pub exposure: DependencyExposure,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(in crate::resolver::kotlin) enum DependencyExposure {
    /// Compile-visible and re-exported to consumers.
    Api,
    /// Compile-visible within this module only.
    Implementation,
    /// Compile-only visibility.
    CompileOnly,
    /// Runtime-only and therefore not compile-visible.
    Runtime,
    /// Unsupported configuration.
    Other,
}

pub(super) struct Settings {
    pub modules: BTreeSet<String>,
    pub directories: BTreeMap<String, PathBuf>,
}

impl Settings {
    pub(super) fn parse(source: &str) -> Self {
        let script = StaticScript::parse(source);
        Self {
            modules: includes(&script),
            directories: project_directories(&script),
        }
    }
}

#[derive(Default)]
pub(super) struct BuildFacts {
    pub dependencies: BTreeSet<ProjectDependency>,
    pub source_edges: BTreeMap<String, BTreeSet<String>>,
}

impl BuildFacts {
    pub(super) fn parse(source: &str) -> Self {
        let script = StaticScript::parse(source);
        let mut edges = BTreeMap::new();
        source_edges(&script, &mut edges);
        Self {
            dependencies: project_dependencies(&script),
            source_edges: edges,
        }
    }
}

pub(super) fn classify_configuration(configuration: &str) -> (DependencyScope, DependencyExposure) {
    const SUFFIXES: [(&str, DependencyExposure); 6] = [
        ("implementation", DependencyExposure::Implementation),
        ("api", DependencyExposure::Api),
        ("compileonly", DependencyExposure::CompileOnly),
        ("runtimeonly", DependencyExposure::Runtime),
        ("compile", DependencyExposure::Implementation),
        ("runtime", DependencyExposure::Runtime),
    ];
    let normalized = configuration.to_ascii_lowercase();
    let Some((suffix, exposure)) = SUFFIXES
        .iter()
        .find(|(suffix, _)| normalized.ends_with(*suffix))
    else {
        return (DependencyScope::Other, DependencyExposure::Other);
    };
    let prefix = &configuration[..configuration.len() - suffix.len()];
    let scope = if prefix.is_empty() || prefix.eq_ignore_ascii_case("main") {
        DependencyScope::Production
    } else if prefix == "test" || prefix.ends_with("Test") {
        DependencyScope::Test(prefix.to_owned())
    } else {
        DependencyScope::Variant(prefix.to_owned())
    };
    (scope, exposure.clone())
}
