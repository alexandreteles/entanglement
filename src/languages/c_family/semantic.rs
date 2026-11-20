use super::{declarator, *};
use crate::languages::syntax::ancestors;
use crate::model::{
    Definition, DefinitionKind, LocalBinding, ModulePath, Reference, ReferenceKind, SemanticStore,
};
use std::collections::BTreeMap;
use tree_sitter::Node;

pub(crate) mod calls;
mod declarations;
mod expressions;

#[derive(Clone, Debug, Default)]
pub(crate) struct Data {
    pub tags: BTreeMap<usize, DefinitionKind>,
    pub aliases: BTreeMap<String, (usize, usize, Vec<String>)>,
    pub classes: BTreeMap<Vec<String>, Vec<Vec<String>>>,
    pub errors: Vec<(usize, usize)>,
    pub symbols: BTreeMap<(usize, String), Symbol>,
    pub calls: BTreeMap<usize, Invocation>,
    pub using: Vec<(usize, usize, Vec<String>, bool)>,
    pub module: Option<String>,
    pub imports: Vec<(usize, usize, String, bool)>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Symbol {
    pub parameters: Option<Vec<String>>,
    pub qualifiers: String,
    pub minimum: usize,
    pub variadic: bool,
    pub body: bool,
    pub internal: bool,
    pub c_linkage: bool,
    pub dynamic: bool,
    pub member: bool,
    pub exported: bool,
    pub result: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Invocation {
    pub constructor: bool,
    pub operator: bool,
    pub arguments: Vec<Option<String>>,
    pub receiver: Option<String>,
    pub indirect: bool,
    pub qualified: bool,
}

/// Capture declarations, bindings and references from one preprocessing view.
pub(crate) fn capture(tree: &Tree, source: &[u8]) -> SemanticStore {
    let mut store = SemanticStore::default();
    let mut data = Data::default();
    let mut damaged = vec![tree.root_node()];
    while let Some(node) = damaged.pop() {
        if node.is_missing() || node.is_error() {
            data.errors.push((node.start_byte(), node.end_byte()));
        } else if node.has_error() {
            damaged.extend((0..node.child_count()).filter_map(|index| node.child(index)));
        }
    }
    let mut pending = vec![tree.root_node()];
    while let Some(node) = pending.pop() {
        match node.kind() {
            "function_definition"
            | "declaration"
            | "field_declaration"
            | "type_definition"
            | "alias_declaration"
            | "struct_specifier"
            | "class_specifier"
            | "enum_specifier"
            | "union_specifier"
            | "lambda_expression"
            | "parameter_declaration"
            | "optional_parameter_declaration" => {
                declarations::capture(node, source, &mut data, &mut store)
            }
            "call_expression" => calls::call(node, source, &mut data, &mut store),
            "init_declarator"
            | "binary_expression"
            | "pointer_expression"
            | "namespace_alias_definition" => {
                expressions::special(node, source, &mut data, &mut store)
            }
            "type_identifier" => calls::type_reference(node, source, &mut data, &mut store),
            "qualified_identifier"
                if node.parent().is_some_and(|parent| {
                    parent.kind() == "function_declarator"
                        && parent.child_by_field_name("declarator") == Some(node)
                }) =>
            {
                if let Some(owner) = node.child_by_field_name("scope") {
                    calls::reference(
                        owner,
                        path(&text(owner, source)),
                        ReferenceKind::Type,
                        source,
                        &mut store,
                    );
                }
            }
            "using_declaration" => {
                let written = text(node, source);
                let value = written
                    .trim_end_matches(';')
                    .trim_start_matches("using ")
                    .trim();
                let target = value.trim_start_matches("namespace ");
                data.using.push((
                    node.start_byte(),
                    scope(node).end_byte(),
                    path(target),
                    value != target,
                ));
            }
            "module_declaration" => {
                let written = text(node, source);
                if let Some((_, name)) = written.split_once("module") {
                    let name = name.trim().trim_end_matches(';').trim();
                    if !name.is_empty() && !name.starts_with(':') {
                        data.module = Some(name.replace(' ', ""));
                    }
                }
            }
            "import_declaration" => {
                let written = text(node, source);
                let name = written
                    .trim_start_matches("import")
                    .trim()
                    .trim_end_matches(';')
                    .trim();
                data.imports.push((
                    node.parent()
                        .filter(|parent| parent.kind() == "export_declaration")
                        .map_or(node.start_byte(), |parent| parent.start_byte()),
                    node.end_byte(),
                    if name.starts_with(['"', '<']) {
                        name.into()
                    } else {
                        name.replace(' ', "")
                    },
                    exported(node),
                ));
            }
            _ => {}
        }
        if !matches!(
            node.kind(),
            "sizeof_expression"
                | "alignof_expression"
                | "decltype"
                | "noexcept_expression"
                | "requires_expression"
                | "requires_clause"
                | "static_assert_declaration"
        ) {
            pending.extend(named_children(node).rev());
        }
    }
    store.semantic.c_family = Some(Box::new(data));
    store
}

fn path(name: &str) -> Vec<String> {
    name.split("::")
        .filter(|part| !part.is_empty())
        .map(|part| part.trim().to_owned())
        .collect()
}

fn scope(node: Node<'_>) -> Node<'_> {
    ancestors(node)
        .skip(1)
        .find(|parent| {
            matches!(
                parent.kind(),
                "compound_statement"
                    | "function_definition"
                    | "lambda_expression"
                    | "field_declaration_list"
                    | "translation_unit"
            )
        })
        .unwrap_or(node)
}

fn exported(node: Node<'_>) -> bool {
    ancestors(node).any(|parent| {
        parent.kind() == "export_declaration"
            || parent.kind() == "module_declaration"
                && parent
                    .child(0)
                    .is_some_and(|child| child.kind() == "export")
    })
}

pub(crate) fn public_member(node: Node<'_>, source: &[u8]) -> bool {
    let Some(mut member) = ancestors(node).find(|parent| {
        parent
            .parent()
            .is_some_and(|outer| outer.kind() == "field_declaration_list")
    }) else {
        return true;
    };
    let class_default = member
        .parent()
        .and_then(|body| body.parent())
        .is_some_and(|class| class.kind() == "class_specifier");
    while let Some(previous) = member.prev_named_sibling() {
        if previous.kind() == "access_specifier" {
            return text(previous, source) != "private";
        }
        member = previous;
    }
    !class_default
}

fn owner_path(node: Node<'_>, source: &[u8]) -> Vec<String> {
    ancestors(node)
        .skip(1)
        .find(|parent| parent.kind() == "function_definition")
        .and_then(|parent| parent.child_by_field_name("declarator"))
        .and_then(|declaration| declarator::callable(declaration, source))
        .map(|mut callable| {
            callable.path.pop();
            callable.path
        })
        .unwrap_or_else(|| declarator::owners(node, source))
}
