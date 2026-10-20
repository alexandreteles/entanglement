//! Compiler-free extraction of static Gradle project and source-set facts.
//! Kotlin source analysis does not require these optional build files.

mod ast;
mod extract;
pub(super) use ast::ScriptKind;
pub(super) use extract::parse;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

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

#[derive(Default)]
pub(super) struct Settings {
    pub modules: BTreeSet<String>,
    pub directories: BTreeMap<String, PathBuf>,
}

#[derive(Default)]
pub(super) struct BuildFacts {
    pub dependencies: BTreeSet<ProjectDependency>,
    pub source_edges: BTreeMap<String, BTreeSet<String>>,
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
