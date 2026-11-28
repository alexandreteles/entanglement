use super::{
    facts::{self, Data, Decl, Using},
    project::Project,
};
use crate::languages::{
    CapturedTree, LanguageHandler, LanguageSpec, ResolutionFamily,
    query::QueryAnalyzer,
    syntax::{named_children, text},
};
use crate::metrics::cyclomatic::FunctionScope;
use crate::model::{DefinitionKind, FileFacts, ModulePath};
use crate::snapshot::Snapshot;
use std::collections::BTreeSet;
use tree_sitter::{Language, Tree};

pub(crate) const LANGUAGE_SPEC: LanguageSpec = LanguageSpec {
    scope: "source.razor",
    resolution_family: ResolutionFamily::CSharp,
    project_manifests: &[],
    file_module_rules: None,
    build,
};

struct Razor(QueryAnalyzer);
fn build(_: &str, language: &Language) -> crate::Result<Box<dyn LanguageHandler>> {
    let query = format!(
        "{}\n{}",
        include_str!("../../queries/razor.scm"),
        tree_sitter_razor_modern::INJECTIONS_QUERY
    );
    Ok(Box::new(Razor(QueryAnalyzer::new(language, &query)?)))
}
impl LanguageHandler for Razor {
    fn capture(&self, tree: &Tree, source: &[u8], tokens: bool) -> crate::Result<CapturedTree> {
        let mut captured = CapturedTree::from(self.0.capture(tree, source, tokens));
        let root = tree.root_node();
        captured.functions.push(FunctionScope {
            name: "<render>".into(),
            range: root.byte_range(),
            line: 1,
        });
        captured
            .injections
            .retain(|request| request.language != "comment");
        for request in &mut captured.injections {
            request.inherit_scope = request.language == "csharp";
            request.publish_exports = false;
        }
        markup::blocks(root, &mut captured.injections);
        Ok(captured)
    }
}

pub(crate) fn bind(file: &mut FileFacts, tree: &Tree, project: &Project, snapshot: &Snapshot) {
    let root = tree.root_node();
    let mut imports = Vec::new();
    let mut namespace = None;
    let mut bases = Vec::new();
    let mut generics = Vec::new();
    let configuration = project
        .owners(&file.target)
        .first()
        .map(|v| &project.views[*v]);
    let template_import = if super::project::extension(&file.path, "razor") {
        "_Imports.razor"
    } else {
        "_ViewImports.cshtml"
    };
    let scope = root.byte_range();
    let mut directories = file
        .target
        .ancestors()
        .skip(1)
        .take_while(|dir| {
            configuration.is_some_and(|v| dir.starts_with(v.project.parent().unwrap_or(dir)))
        })
        .collect::<Vec<_>>();
    directories.reverse();
    for directory in directories {
        if let Ok(Some(bytes)) = snapshot.read(&directory.join(template_import)) {
            for line in String::from_utf8_lossy(&bytes).lines() {
                if let Some(name) = line.trim().strip_prefix("@namespace ") {
                    namespace = Some(qualified_directory(name.trim(), directory, &file.target));
                }
                if let Some(name) = line.trim().strip_prefix("@using ") {
                    imports.push(name.trim().trim_end_matches(';').to_owned());
                }
            }
        }
    }
    for node in named_children(root).filter(|n| n.kind().ends_with("_directive")) {
        let directive = text(node, &file.source);
        let Some((_, argument)) = directive.split_once(char::is_whitespace) else {
            continue;
        };
        let argument = argument.trim().trim_end_matches(';').to_owned();
        match node.kind() {
            "razor_namespace_directive" => namespace = Some(argument),
            "razor_typeparam_directive" => {
                if let Some(name) = node.child_by_field_name("name") {
                    generics.push(text(name, &file.source));
                }
            }
            "razor_using_directive" => imports.push(argument),
            "razor_implements_directive" | "razor_inherits_directive" => bases.push(argument),
            _ => {}
        }
    }
    let namespace = namespace.unwrap_or_else(|| {
        configuration.map_or(String::new(), |v| {
            qualified_directory(
                &v.namespace,
                v.project.parent().unwrap_or(&v.project),
                &file.target,
            )
        })
    });
    let mut name = file
        .path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .replace('-', "_");
    if super::project::extension(&file.path, "cshtml") {
        let base = configuration
            .and_then(|v| v.project.parent())
            .unwrap_or_else(|| file.path.parent().unwrap_or(&file.path));
        let relative = file.path.strip_prefix(base).unwrap_or(&file.path);
        name = format!("<view {}>", relative.display());
    }
    let declaration = Decl {
        fingerprint: file.contexts[0].semantic.structure_fingerprint.clone(),
        execution: None,
        range: scope.clone(),
        name,
        owner: namespace.clone(),
        namespace: namespace.clone(),
        category: "class".into(),
        kind: DefinitionKind::Struct,
        is_type: true,
        scope: scope.clone(),
        visibility: "public".into(),
        modifiers: BTreeSet::from(["partial".into()]),
        parameters: Vec::new(),
        generics,
        ty: String::new(),
        types: Vec::new(),
        bases,
        extension: None,
        explicit: None,
        body: None,
    };
    let owner = declaration.qualified();
    let imports = imports
        .iter()
        .map(|name| {
            let (alias, target) = name.split_once('=').unwrap_or(("", name));
            Using {
                position: 0,
                scope: scope.clone(),
                namespace: namespace.clone(),
                alias: alias.trim().into(),
                target: target.trim_start_matches("static ").trim().into(),
                global: false,
                is_static: target.starts_with("static "),
            }
        })
        .collect::<Vec<_>>();
    for store in &mut file.contexts {
        let Some(data) = store.semantic.csharp.as_deref_mut() else {
            continue;
        };
        for decl in data.declarations.values_mut() {
            decl.owner = if decl.owner.is_empty() {
                owner.clone()
            } else {
                facts::qualify(&owner, &decl.owner)
            };
            decl.namespace = namespace.clone();
        }
        for usage in data.uses.values_mut() {
            usage.owner = owner.clone();
            usage.namespace = namespace.clone();
        }
        for definition in &mut store.definitions {
            if let Some(decl) = data.declarations.get(&definition.start_byte) {
                definition.module = ModulePath(decl.owner.split('.').map(str::to_owned).collect());
                definition.kind = decl.kind.clone();
            }
        }
        data.imports.extend(imports.iter().cloned());
    }
    let existing = file
        .contexts
        .iter()
        .filter_map(|s| s.semantic.csharp.as_deref())
        .flat_map(|d| d.uses.keys().copied())
        .collect::<BTreeSet<_>>();
    let host = &mut file.contexts[0];
    if host.semantic.structure_errors {
        host.semantic.structure_errors = markup::has_host_error(root, &file.analysis.injections);
    }
    let mut data = Data::default();
    host::capture(
        root,
        &file.source,
        &namespace,
        &owner,
        host,
        &mut data,
        &existing,
    );
    data.imports = imports;
    data.declarations.insert(scope.start, declaration);
    host.semantic.csharp = Some(Box::new(data));
}
fn qualified_directory(namespace: &str, base: &std::path::Path, file: &std::path::Path) -> String {
    file.parent()
        .and_then(|p| p.strip_prefix(base).ok())
        .into_iter()
        .flat_map(|p| p.components())
        .fold(namespace.into(), |s, component| {
            facts::qualify(&s, &component.as_os_str().to_string_lossy())
        })
}
pub(crate) mod fragment;
mod host;
mod markup;
