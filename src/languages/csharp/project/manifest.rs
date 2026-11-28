use super::conditions::{Properties, condition, expand};
use super::items::{CompileRule, path};
use crate::snapshot::Snapshot;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

#[derive(Default)]
pub(super) struct Evaluation {
    pub properties: Properties,
    pub rules: Vec<CompileRule>,
    pub references: Vec<PathBuf>,
    pub usings: Vec<(String, String, bool)>,
    pub limitations: BTreeSet<String>,
    pub sdk: bool,
}

impl Evaluation {
    pub(super) fn read(
        project: &Path,
        configuration: &str,
        framework: &str,
        snapshot: &Snapshot,
    ) -> Self {
        let directory = project.parent().unwrap_or(project);
        let mut data = Self::default();
        for (key, value) in [
            ("configuration", configuration),
            ("platform", "AnyCPU"),
            ("baseoutputpath", "bin"),
            ("baseintermediateoutputpath", "obj"),
            ("targetframework", framework),
            ("msbuildprojectdirectory", directory.to_str().unwrap_or("")),
            (
                "msbuildprojectname",
                project.file_stem().and_then(|s| s.to_str()).unwrap_or(""),
            ),
        ] {
            data.properties.insert(key.into(), value.into());
        }
        data.sdk = snapshot.read(project).ok().flatten().is_some_and(|bytes| {
            std::str::from_utf8(&bytes)
                .ok()
                .and_then(|s| roxmltree::Document::parse(s.trim_start_matches('\u{feff}')).ok())
                .is_some_and(|doc| doc.root_element().attribute("Sdk").is_some())
        });
        if data.sdk {
            data.properties
                .insert("defineconstants".into(), "TRACE".into());
        }
        let files = [
            nearest(directory, "Directory.Build.props", snapshot),
            Some(project.to_owned()),
            nearest(directory, "Directory.Build.targets", snapshot),
        ];
        for properties in [true, false] {
            for file in files.iter().flatten() {
                data.document(file, directory, snapshot, properties, &mut BTreeSet::new());
            }
        }
        data
    }

    fn document(
        &mut self,
        file: &Path,
        base: &Path,
        snapshot: &Snapshot,
        properties: bool,
        active: &mut BTreeSet<PathBuf>,
    ) {
        if !active.insert(file.to_owned()) {
            self.limitations.insert("msbuild_import_cycle".into());
            return;
        }
        let bytes = snapshot.read(file).ok().flatten();
        let document = bytes
            .as_deref()
            .and_then(|b| std::str::from_utf8(b).ok())
            .and_then(|s| roxmltree::Document::parse(s.trim_start_matches('\u{feff}')).ok());
        if let Some(document) = document {
            let root = document.root_element();
            self.sdk |= root.attribute("Sdk").is_some();
            let directory = file.parent().unwrap_or(base);
            let previous = self.properties.insert(
                "msbuildthisfiledirectory".into(),
                format!("{}/", directory.display()),
            );
            self.group(root, base, directory, snapshot, properties, active);
            if let Some(value) = previous {
                self.properties
                    .insert("msbuildthisfiledirectory".into(), value);
            } else {
                self.properties.remove("msbuildthisfiledirectory");
            }
        } else {
            self.limitations.insert("msbuild_import".into());
        }
        active.remove(file);
    }

    fn group(
        &mut self,
        group: roxmltree::Node<'_, '_>,
        base: &Path,
        directory: &Path,
        snapshot: &Snapshot,
        properties: bool,
        active: &mut BTreeSet<PathBuf>,
    ) {
        for node in group.children().filter(|n| n.is_element()) {
            if !self.enabled(node, directory, snapshot) {
                continue;
            }
            match node.tag_name().name() {
                "Import" if node.attribute("Sdk").is_some() => self.sdk = true,
                "Import" => {
                    if let Some(name) = node
                        .attribute("Project")
                        .and_then(|s| expand(s, &self.properties))
                    {
                        self.document(&path(directory, &name), base, snapshot, properties, active);
                    } else {
                        self.limitations.insert("msbuild_import".into());
                    }
                }
                "PropertyGroup" if properties => self.properties(node, directory, snapshot),
                "ItemGroup" if !properties => self.items(node, base, directory, snapshot),
                "ImportGroup" => self.group(node, base, directory, snapshot, properties, active),
                "Choose" => {
                    let choice = node
                        .children()
                        .filter(|child| child.has_tag_name("When"))
                        .find(|child| self.enabled(*child, directory, snapshot))
                        .or_else(|| {
                            node.children()
                                .find(|child| child.has_tag_name("Otherwise"))
                        });
                    if let Some(branch) = choice {
                        self.group(branch, base, directory, snapshot, properties, active);
                    }
                }
                "Target" if node.descendants().any(|n| n.has_tag_name("Compile")) => {
                    self.limitations.insert("generated_sources".into());
                }
                _ => {}
            }
        }
    }

    fn enabled(&mut self, node: roxmltree::Node<'_, '_>, dir: &Path, snapshot: &Snapshot) -> bool {
        condition(node.attribute("Condition"), &self.properties, dir, snapshot).unwrap_or_else(
            || {
                self.limitations.insert("msbuild_condition".into());
                false
            },
        )
    }

    fn properties(&mut self, group: roxmltree::Node<'_, '_>, dir: &Path, snapshot: &Snapshot) {
        for node in group.children().filter(|n| n.is_element()) {
            let key = node.tag_name().name().to_ascii_lowercase();
            if self.enabled(node, dir, snapshot)
                && !(matches!(
                    key.as_str(),
                    "configuration" | "platform" | "targetframework"
                ) && self.properties.get(&key).is_some_and(|v| !v.is_empty()))
                && let Some(value) = expand(node.text().unwrap_or("").trim(), &self.properties)
            {
                self.properties.insert(key, value);
            }
        }
    }

    fn items(
        &mut self,
        group: roxmltree::Node<'_, '_>,
        base: &Path,
        dir: &Path,
        snapshot: &Snapshot,
    ) {
        for node in group.children().filter(|n| n.is_element()) {
            if !self.enabled(node, dir, snapshot) {
                continue;
            }
            let attribute = |key| {
                node.attribute(key)
                    .and_then(|s| expand(s, &self.properties))
            };
            match node.tag_name().name() {
                "Compile" => {
                    if let Some(value) = attribute("Include").or_else(|| attribute("Remove")) {
                        if let Some(rule) = CompileRule::new(
                            base,
                            &value,
                            &attribute("Exclude").unwrap_or_default(),
                            node.attribute("Include").is_some(),
                        ) {
                            self.rules.push(rule);
                        } else {
                            self.limitations.insert("msbuild_compile_items".into());
                        }
                    }
                }
                "ProjectReference"
                    if node
                        .children()
                        .find(|n| n.has_tag_name("ReferenceOutputAssembly"))
                        .and_then(|n| n.text())
                        != Some("false") =>
                {
                    self.references.extend(
                        attribute("Include")
                            .iter()
                            .flat_map(|v| v.split(';'))
                            .map(|s| path(base, s)),
                    );
                }
                "Using" => {
                    if let Some(name) = attribute("Include") {
                        self.usings.push((
                            attribute("Alias").unwrap_or_default(),
                            name,
                            attribute("Static").as_deref() == Some("True"),
                        ));
                    }
                }
                "Analyzer" => {
                    self.limitations.insert("generated_sources".into());
                }
                _ => {}
            }
        }
    }
}

fn nearest(dir: &Path, name: &str, snapshot: &Snapshot) -> Option<PathBuf> {
    dir.ancestors()
        .map(|p| p.join(name))
        .find(|path| snapshot.read(path).ok().flatten().is_some())
}
