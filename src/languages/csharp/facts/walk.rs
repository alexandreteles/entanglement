use super::{
    Binding, Data, Decl, Expression, Using, declarations, expressions, qualify, references,
};
use crate::languages::syntax::{named_children, text};
use crate::metrics::cyclomatic::FunctionScope;
use crate::model::{Definition, ModulePath, SemanticStore};
use std::collections::{BTreeSet, HashMap};
use std::ops::Range;
use tree_sitter::Node;

#[derive(Clone, Default)]
pub(super) struct Context {
    pub namespace: String,
    pub owner: String,
    pub scope: Range<usize>,
    pub namespace_scope: Range<usize>,
    pub interface: bool,
    pub in_type: bool,
    pub callable: Option<usize>,
    pub extension: Option<String>,
    pub contract: bool,
    pub top_level: Option<Range<usize>>,
}

pub(crate) fn capture(root: Node<'_>, source: &[u8], scopes: &[FunctionScope]) -> SemanticStore {
    let mut store = SemanticStore::default();
    let mut data = Data::default();
    if root.has_error() {
        let mut pending = vec![root];
        while let Some(node) = pending.pop() {
            if node.is_error() || node.is_missing() {
                data.errors.push(node.byte_range());
            } else if node.has_error() {
                pending.extend(node.children(&mut node.walk()));
            }
        }
    }
    let functions = scopes
        .iter()
        .map(|f| (f.range.start, f.range.end))
        .collect::<HashMap<_, _>>();
    let context = Context {
        scope: root.byte_range(),
        namespace_scope: root.byte_range(),
        top_level: scopes
            .iter()
            .find(|scope| scope.name == "<Main>")
            .map(|scope| scope.range.clone()),
        ..Default::default()
    };
    visit(root, source, &context, &functions, &mut data, &mut store);
    store.semantic.csharp = Some(Box::new(data));
    store
}

fn visit(
    node: Node<'_>,
    source: &[u8],
    parent: &Context,
    functions: &HashMap<usize, usize>,
    data: &mut Data,
    store: &mut SemanticStore,
) {
    let mut ctx = parent.clone();
    if node.kind() == "global_statement"
        && let Some(range) = &ctx.top_level
        && range.contains(&node.start_byte())
    {
        ctx.callable = Some(range.start);
    }
    if node.kind() == "using_directive" {
        let mut cursor = node.walk();
        if let Some(target) = named_children(node).rfind(|n| n.kind() != "comment") {
            references::import(target, source, &ctx, data, store);
            data.imports.push(Using {
                position: node.start_byte(),
                scope: ctx.namespace_scope.clone(),
                namespace: ctx.namespace.clone(),
                alias: node
                    .child_by_field_name("name")
                    .map(|n| text(n, source))
                    .unwrap_or_default(),
                target: text(target, source),
                global: node.child(0).is_some_and(|n| n.kind() == "global"),
                is_static: node.children(&mut cursor).any(|n| n.kind() == "static"),
            });
        }
        return;
    }
    if matches!(
        node.kind(),
        "file_scoped_namespace_declaration" | "namespace_declaration"
    ) && let Some(name) = node.child_by_field_name("name")
    {
        ctx.namespace = qualify(&ctx.namespace, &text(name, source));
        ctx.owner = ctx.namespace.clone();
        ctx.namespace_scope = node.byte_range();
        ctx.scope = node.byte_range();
    }
    if node.kind() == "extension_declaration" {
        ctx.extension = named_children(node)
            .find(|n| n.kind() == "receiver_parameter")
            .and_then(|n| n.child_by_field_name("type"))
            .map(|n| text(n, source));
    }
    if let Some(decl) = declarations::read(node, source, &ctx) {
        store.definitions.push(definition(&decl));
        if decl.is_type {
            ctx.owner = decl.qualified();
            ctx.interface = decl.category == "interface";
            ctx.in_type = true;
        }
        ctx.contract = decl.public() && (!parent.in_type || parent.contract);
        if parent.callable.is_none()
            && matches!(
                decl.category.as_str(),
                "method" | "constructor" | "local" | "operator"
            )
        {
            ctx.scope = node.byte_range();
        }
        data.declarations.insert(decl.range.start, decl);
    }
    if functions.get(&node.start_byte()) == Some(&node.end_byte()) {
        ctx.callable = Some(node.start_byte());
        ctx.scope = node.byte_range();
    }
    if matches!(
        node.kind(),
        "block"
            | "query_expression"
            | "catch_clause"
            | "foreach_statement"
            | "for_statement"
            | "lambda_expression"
            | "anonymous_method_expression"
    ) {
        ctx.scope = node.byte_range();
        ctx.contract = false;
    }
    if node.kind() == "arrow_expression_clause" {
        ctx.contract = false;
    }
    binding(node, source, &ctx, data);
    references::capture(node, source, &ctx, data, store);
    for child in named_children(node) {
        visit(child, source, &ctx, functions, data, store);
    }
}

fn definition(decl: &Decl) -> Definition {
    Definition {
        name: decl.name.clone(),
        kind: decl.kind.clone(),
        module: ModulePath(
            decl.owner
                .split('.')
                .filter(|v| !v.is_empty())
                .map(str::to_owned)
                .collect(),
        ),
        start_byte: decl.range.start,
        end_byte: decl.range.end,
        scope_start: decl.scope.start,
        scope_end: decl.scope.end,
        is_public: decl.public(),
        inline_module: false,
        external_module: false,
        context_id: 0,
        declaration: decl.execution.clone().map(|execution| {
            Box::new(crate::graph::DeclarationFacts {
                execution: Some(execution),
                ..Default::default()
            })
        }),
    }
}

pub(super) fn visibility(modifiers: &BTreeSet<String>, ctx: &Context, is_type: bool) -> String {
    let mut words = ["private", "protected", "internal", "public", "file"]
        .into_iter()
        .filter(|s| modifiers.contains(*s))
        .collect::<Vec<_>>();
    if words.is_empty() {
        words.push(if ctx.interface {
            "public"
        } else if is_type && !ctx.in_type {
            "internal"
        } else {
            "private"
        });
    }
    words.join(" ")
}

fn binding(node: Node<'_>, source: &[u8], ctx: &Context, data: &mut Data) {
    let parent = node.parent();
    let entry = match node.kind() {
        "parameter"
        | "receiver_parameter"
        | "catch_declaration"
        | "declaration_pattern"
        | "declaration_expression"
        | "from_clause"
        | "join_clause"
        | "let_clause" => node.child_by_field_name("name").map(|n| {
            (
                n,
                node.child_by_field_name("type")
                    .map_or(Expression::Unknown, |ty| Expression::Type(text(ty, source))),
            )
        }),
        "implicit_parameter" => Some((node, Expression::Unknown)),
        "foreach_statement" => node
            .child_by_field_name("left")
            .filter(|n| n.kind() == "identifier")
            .map(|n| {
                (
                    n,
                    node.child_by_field_name("type")
                        .map_or(Expression::Unknown, |ty| Expression::Type(text(ty, source))),
                )
            }),
        "variable_declarator"
            if parent.and_then(|p| p.parent()).is_some_and(|p| {
                !matches!(p.kind(), "field_declaration" | "event_field_declaration")
            }) =>
        {
            let ty = parent.and_then(|p| p.child_by_field_name("type"));
            node.child_by_field_name("name").map(|n| {
                (
                    n,
                    ty.filter(|v| text(*v, source) != "var").map_or_else(
                        || {
                            named_children(node)
                                .find(|v| v.id() != n.id())
                                .map_or(Expression::Unknown, |v| expressions::expression(v, source))
                        },
                        |v| Expression::Type(text(v, source)),
                    ),
                )
            })
        }
        _ => None,
    };
    if let Some((name, value)) = entry {
        data.bindings.push(Binding {
            name: expressions::simple(name, source),
            scope: ctx.scope.clone(),
            position: name.start_byte(),
            value,
        });
    }
}
