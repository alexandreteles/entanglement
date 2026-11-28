use super::{View, framework, items, manifest::Evaluation};
use crate::snapshot::Snapshot;
use std::{collections::BTreeSet, path::Path};

pub(super) fn read(project: &Path, snapshot: &Snapshot) -> Vec<View> {
    let base = Evaluation::read(project, "", "", snapshot);
    let get = |key: &str| base.properties.get(key).map(String::as_str);
    let configs = get("configurations")
        .unwrap_or("Debug;Release")
        .split(';')
        .map(str::trim);
    let mut result = Vec::new();
    for configuration in configs {
        let evaluation = Evaluation::read(project, configuration, "", snapshot);
        let targets = evaluation
            .properties
            .get("targetframework")
            .filter(|v| !v.is_empty())
            .or(evaluation.properties.get("targetframeworks"))
            .cloned()
            .unwrap_or_default();
        for framework in targets.split(';').map(str::trim) {
            let data = Evaluation::read(project, configuration, framework, snapshot);
            let property = |name| data.properties.get(name).map(String::as_str).unwrap_or("");
            let mut symbols = if data.sdk
                && !property("disableimplicitframeworkdefines").eq_ignore_ascii_case("true")
            {
                framework::symbols(framework, property("targetplatformversion"))
            } else {
                BTreeSet::new()
            };
            symbols.extend(
                property("defineconstants")
                    .split([';', ',', ' '])
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned),
            );
            if data.sdk
                && !property("disableimplicitconfigurationdefines").eq_ignore_ascii_case("true")
            {
                symbols.insert(
                    configuration
                        .to_ascii_uppercase()
                        .replace(['-', '.', ' '], "_"),
                );
            }
            let default_items = data.sdk
                && !["enabledefaultcompileitems", "enabledefaultitems"]
                    .iter()
                    .any(|k| property(k).eq_ignore_ascii_case("false"));
            let mut usings = data.usings.clone();
            if matches!(
                property("implicitusings").to_ascii_lowercase().as_str(),
                "true" | "enable"
            ) {
                usings.extend(
                    [
                        "System",
                        "System.Collections.Generic",
                        "System.IO",
                        "System.Linq",
                        "System.Net.Http",
                        "System.Threading",
                        "System.Threading.Tasks",
                    ]
                    .map(|name| (String::new(), name.into(), false)),
                );
            }
            result.push(View {
                excludes: items::patterns(
                    project.parent().unwrap_or(project),
                    &format!(
                        "{};{};{}/**;{}/**",
                        property("defaultitemexcludes"),
                        property("defaultitemexcludesinprojectfolder"),
                        property("baseoutputpath").trim_end_matches(['/', '\\']),
                        property("baseintermediateoutputpath").trim_end_matches(['/', '\\'])
                    ),
                )
                .unwrap_or_default(),
                project: project.into(),
                configuration: configuration.into(),
                framework: framework.into(),
                symbols,
                references: data.references,
                usings,
                limits: data.limitations,
                default_items,
                rules: data.rules,
            });
        }
    }
    result
}
