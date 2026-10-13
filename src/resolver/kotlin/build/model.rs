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

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum GradleBlock {
    Kotlin,
    Android,
    SourceSets,
    BuildTypes,
    ProductFlavors,
    SourceSet(String),
    Variant(String),
    Other,
}
