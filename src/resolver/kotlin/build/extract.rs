//! Shared Gradle policy over Kotlin and Groovy syntax trees.

use super::ast::{Call, ScriptKind, children, identifier, literal, project_path, text, valid_name};
use super::{BuildFacts, DependencyScope, ProjectDependency, Settings, classify_configuration};
use std::path::PathBuf;
use tree_sitter::Node;

#[derive(Default)]
struct ScriptFacts {
    settings: Settings,
    build: BuildFacts,
}

#[derive(Clone)]
enum Scope {
    Root,
    Kotlin,
    Android,
    SourceSets(bool),
    Variants,
    SourceSet {
        name: String,
        dependencies: bool,
        edges: bool,
    },
    Dependencies(DependencyScope),
}

pub(in crate::resolver::kotlin) fn parse(source: &str, kind: ScriptKind) -> (Settings, BuildFacts) {
    let mut facts = ScriptFacts::default();
    if let Some(tree) = kind.parse(source) {
        facts.walk(tree.root_node(), source, Scope::Root);
    }
    (facts.settings, facts.build)
}

impl ScriptFacts {
    fn walk(&mut self, body: Node<'_>, source: &str, scope: Scope) {
        for node in children(body) {
            match (&scope, node.kind()) {
                (Scope::Root, "assignment") => self.settings.directories.extend(
                    project_directory(node, source).map(|(module, path)| (module.into(), path)),
                ),
                (Scope::SourceSets(_), "property_declaration") => {
                    if let Some((name, call)) = declared_source_set(node, source)
                        && let Some(body) = call.body
                    {
                        self.walk(
                            body,
                            source,
                            Scope::SourceSet {
                                name: name.into(),
                                dependencies: ["getting", "creating"].contains(&call.name)
                                    && call.args.is_empty(),
                                edges: true,
                            },
                        );
                    }
                }
                _ => {
                    if let Some(call) = Call::read(node, source) {
                        self.call(call, source, &scope);
                    }
                }
            }
        }
    }

    fn call(&mut self, call: Call<'_, '_>, source: &str, scope: &Scope) {
        match scope {
            Scope::Root if call.name == "include" && call.receiver.is_none() => {
                self.settings.modules.extend(
                    call.args
                        .iter()
                        .filter_map(|node| literal(*node, source))
                        .filter(|value| project_path(value))
                        .map(str::to_owned),
                );
            }
            Scope::Dependencies(scope) if call.receiver.is_none() => {
                self.dependencies(&call, source, scope);
            }
            Scope::SourceSet {
                name, edges: true, ..
            } if call.name == "dependsOn" && call.receiver.is_none() => {
                self.source_edge(name, &call, source);
            }
            Scope::SourceSets(_) if call.name == "dependsOn" => {
                if let Some(child) = call.receiver {
                    self.source_edge(child, &call, source);
                }
            }
            _ => {}
        }
        if let Some(body) = call.body
            && let Some(next) = child_scope(scope, &call, source)
        {
            self.walk(body, source, next);
        }
    }

    fn dependencies(&mut self, call: &Call<'_, '_>, source: &str, scope: &DependencyScope) {
        let (declared_scope, exposure) = classify_configuration(call.name);
        let scope = match scope {
            DependencyScope::Production => declared_scope,
            scope => scope.clone(),
        };
        self.build
            .dependencies
            .extend(call.args.iter().filter_map(|node| {
                let project = Call::read(*node, source)
                    .filter(|call| call.name == "project" && call.receiver.is_none())?;
                let path = project.string(source).filter(|path| project_path(path))?;
                Some(ProjectDependency {
                    path: path.into(),
                    configuration: call.name.into(),
                    scope: scope.clone(),
                    exposure: exposure.clone(),
                })
            }));
    }

    fn source_edge(&mut self, child: &str, call: &Call<'_, '_>, source: &str) {
        let Some(first) = call.args.first() else {
            return;
        };
        // Preserve the existing static extractor's first-token rule.
        let mut head = *first;
        while let Some(child) = head
            .named_child(0)
            .filter(|child| child.start_byte() == first.start_byte())
        {
            head = child;
        }
        let parent = identifier(head, source).or_else(|| call.string(source));
        if let Some(parent) = parent.filter(|name| valid_name(name)) {
            self.build
                .source_edges
                .entry(child.into())
                .or_default()
                .insert(parent.into());
        }
    }
}

fn child_scope(scope: &Scope, call: &Call<'_, '_>, source: &str) -> Option<Scope> {
    match (scope, call.receiver, call.name) {
        (Scope::Root, None, "kotlin") => Some(Scope::Kotlin),
        (Scope::Root, None, "android") => Some(Scope::Android),
        (Scope::Root, None, "dependencies") => {
            Some(Scope::Dependencies(DependencyScope::Production))
        }
        (Scope::Kotlin, None, "sourceSets") => Some(Scope::SourceSets(true)),
        (Scope::Android, None, "sourceSets") => Some(Scope::SourceSets(false)),
        (Scope::Android, None, "buildTypes" | "productFlavors") => Some(Scope::Variants),
        (
            Scope::SourceSet {
                name,
                dependencies: true,
                ..
            },
            None,
            "dependencies",
        ) => Some(Scope::Dependencies(DependencyScope::Variant(name.clone()))),
        (Scope::SourceSets(true), Some(name), "dependencies") => {
            Some(Scope::Dependencies(DependencyScope::Variant(name.into())))
        }
        (Scope::SourceSets(_) | Scope::Variants, None, _) => {
            named_source_set(call, source).map(|name| Scope::SourceSet {
                name,
                dependencies: true,
                edges: matches!(scope, Scope::SourceSets(_))
                    && matches!(call.name, "getByName" | "named"),
            })
        }
        _ => None,
    }
}

fn named_source_set(call: &Call<'_, '_>, source: &str) -> Option<String> {
    let name = match call.name {
        "getByName" | "named" | "create" | "maybeCreate" => call.string(source)?,
        "getting" | "creating" => return None,
        name if call.args.is_empty() => name,
        _ => return None,
    };
    valid_name(name).then(|| name.into())
}

fn declared_source_set<'tree, 'source>(
    node: Node<'tree>,
    source: &'source str,
) -> Option<(&'source str, Call<'tree, 'source>)> {
    let variable = children(node).find(|child| child.kind() == "variable_declaration")?;
    let name = identifier(variable.named_child(0)?, source)?;
    valid_name(name).then_some(())?;
    let delegate = children(node).find(|child| child.kind() == "property_delegate")?;
    let call = Call::read(delegate.named_child(0)?, source)?;
    Some((name, call))
}

fn project_directory<'a>(node: Node<'_>, source: &'a str) -> Option<(&'a str, PathBuf)> {
    let left = node.named_child(0)?;
    let property = children(left).last()?;
    (text(property, source).trim_start_matches('.') == "projectDir").then_some(())?;
    let project = Call::read(left.named_child(0)?, source)?;
    (project.name == "project" && project.receiver.is_none()).then_some(())?;
    let module = project.string(source).filter(|path| project_path(path))?;
    let file = Call::read(node.named_child(1)?, source)?;
    (file.name == "file" && file.receiver.is_none()).then_some(())?;
    let path = PathBuf::from(file.string(source)?);
    (!path.is_absolute()).then_some((module, path))
}
