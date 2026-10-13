use super::model::{DependencyExposure, DependencyScope};

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
