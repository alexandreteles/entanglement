//! Compiler-free extraction of statically proven Gradle project and source-set facts.
//! Shared syntax and scope evidence is prepared once in `StaticScript` and used
//! by the narrow settings, dependency, and source-set extractors below.

#[path = "build/blocks.rs"]
mod blocks;
#[path = "build/configurations.rs"]
mod configurations;
#[path = "build/dependencies.rs"]
mod dependencies;
#[path = "build/lexer.rs"]
mod lexer;
#[path = "build/model.rs"]
mod model;
#[path = "build/scopes.rs"]
mod scopes;
#[path = "build/script.rs"]
mod script;
#[path = "build/settings.rs"]
mod settings;
#[path = "build/source_set_edges.rs"]
mod source_set_edges;
#[path = "build/source_set_items.rs"]
mod source_set_items;
#[path = "build/source_sets.rs"]
mod source_sets;
#[path = "build/syntax.rs"]
mod syntax;
#[cfg(test)]
#[path = "../../../tests/unit/kotlin_build.rs"]
mod tests;

pub(in crate::resolver::kotlin) use dependencies::parse_project_dependency_details;
pub(in crate::resolver::kotlin) use model::{
    DependencyExposure, DependencyScope, ProjectDependency,
};
pub(in crate::resolver::kotlin) use settings::{parse_includes, parse_project_directories};
pub(in crate::resolver::kotlin) use source_sets::parse_source_edges;
