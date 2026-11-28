use super::expressions::{arguments, callee, expression, simple};
use super::walk::Context;
use super::{Data, Expression, Usage};
use crate::languages::syntax::{named_children, text};
use crate::model::{ModulePath, Reference, ReferenceKind, SemanticStore};
use crate::structure::model::RelationshipKind;
use tree_sitter::Node;

pub(super) fn capture(
    node: Node<'_>,
    source: &[u8],
    ctx: &Context,
    data: &mut Data,
    store: &mut SemanticStore,
) {
    let field = |key| node.child_by_field_name(key);
    let parent = node.parent();
    let mut usage = Usage {
        attribute: false,
        write: parent.is_some_and(|p| {
            p.kind() == "assignment_expression" && p.child_by_field_name("left") == Some(node)
        }),
        range: node.byte_range(),
        name: String::new(),
        namespace: ctx.namespace.clone(),
        owner: ctx.owner.clone(),
        receiver: Expression::Unknown,
        arguments: Vec::new(),
        arity: None,
        kind: ReferenceKind::Type,
        relation: RelationshipKind::Type,
        constructor: false,
        contract: ctx.contract,
    };
    let name_node = match node.kind() {
        "invocation_expression" => {
            let Some(function) = field("function") else {
                return;
            };
            let Some(name) = callee(function) else { return };
            usage.kind = ReferenceKind::Call;
            usage.relation = RelationshipKind::Call;
            usage.receiver = function.child_by_field_name("expression").map_or_else(
                || {
                    if function.kind() == "conditional_access_expression" {
                        return function
                            .child_by_field_name("condition")
                            .map_or(Expression::Unknown, |n| expression(n, source));
                    }
                    if function.kind() == "member_binding_expression" {
                        return node
                            .parent()
                            .and_then(|p| p.child_by_field_name("condition"))
                            .map_or(Expression::Unknown, |n| expression(n, source));
                    }
                    Expression::Unknown
                },
                |n| expression(n, source),
            );
            usage.arguments = arguments(field("arguments"), source);
            name
        }
        "element_access_expression" => {
            let (Some(receiver), Some(subscript)) = (field("expression"), field("subscript"))
            else {
                return;
            };
            usage.kind = ReferenceKind::Value;
            usage.relation = RelationshipKind::Field;
            usage.receiver = expression(receiver, source);
            usage.arguments = arguments(Some(subscript), source);
            subscript
        }
        "object_creation_expression" => {
            let Some(ty) = field("type") else { return };
            usage.constructor = true;
            usage.relation = RelationshipKind::Call;
            usage.kind = ReferenceKind::Call;
            usage.arguments = arguments(field("arguments"), source);
            ty
        }
        "member_access_expression"
            if !parent.is_some_and(|p| {
                p.kind() == "invocation_expression"
                    && p.child_by_field_name("function") == Some(node)
            }) =>
        {
            let Some(name) = field("name") else { return };
            usage.kind = ReferenceKind::Value;
            usage.relation = RelationshipKind::Field;
            usage.receiver =
                field("expression").map_or(Expression::Unknown, |n| expression(n, source));
            name
        }
        "identifier"
        | "qualified_name"
        | "alias_qualified_name"
        | "generic_name"
        | "predefined_type" => {
            let Some(parent) = parent else { return };
            if matches!(parent.kind(), "base_list" | "record_base") {
                usage.relation = RelationshipKind::Inherit;
            } else if parent.kind() == "attribute"
                && parent.child_by_field_name("name") == Some(node)
            {
                usage.attribute = true;
            } else if !type_position(parent, node) {
                if node.kind() != "identifier" || !super::accessors::value_position(parent, node) {
                    return;
                }
                usage.kind = ReferenceKind::Value;
                usage.relation = RelationshipKind::Field;
                let name = simple(node, source);
                if data
                    .bindings
                    .iter()
                    .rev()
                    .any(|b| b.name == name && b.scope.contains(&node.start_byte()))
                {
                    return;
                }
            }
            node
        }
        _ => return,
    };
    usage.range = name_node.byte_range();
    usage.name = if node.kind() == "element_access_expression" {
        "this".into()
    } else if usage.kind != ReferenceKind::Type
        && !usage.constructor
        && name_node.kind() == "generic_name"
    {
        usage.arity = named_children(name_node)
            .find(|n| n.kind() == "type_argument_list")
            .map(|n| named_children(n).count());
        named_children(name_node)
            .next()
            .map(|n| simple(n, source))
            .unwrap_or_default()
    } else {
        text(name_node, source).trim_start_matches('@').to_owned()
    };
    if usage.name == "var" {
        return;
    }
    emit(usage, ctx, data, store);
}

pub(super) fn import(
    target: Node<'_>,
    source: &[u8],
    ctx: &Context,
    data: &mut Data,
    store: &mut SemanticStore,
) {
    emit(
        Usage {
            attribute: false,
            write: false,
            range: target.byte_range(),
            name: text(target, source),
            namespace: ctx.namespace.clone(),
            owner: ctx.owner.clone(),
            receiver: Expression::Unknown,
            arguments: Vec::new(),
            arity: None,
            kind: ReferenceKind::Type,
            relation: RelationshipKind::Import,
            constructor: false,
            contract: false,
        },
        ctx,
        data,
        store,
    );
}

fn emit(usage: Usage, ctx: &Context, data: &mut Data, store: &mut SemanticStore) {
    let mut path = match &usage.receiver {
        Expression::Name(name) => vec![name.clone()],
        _ => Vec::new(),
    };
    path.push(usage.name.clone());
    let reference = Reference {
        path,
        module: ModulePath(
            ctx.namespace
                .split('.')
                .filter(|v| !v.is_empty())
                .map(str::to_owned)
                .collect(),
        ),
        kind: usage.kind,
        start_byte: usage.range.start,
        end_byte: usage.range.end,
        scope_start: ctx.scope.start,
        scope_end: ctx.scope.end,
        call_owner: ctx.callable,
        execution_owner: ctx.callable,
        access: None,
        context_id: 0,
    };
    store.references.push(reference);
    data.uses
        .insert((usage.range.start, usage.range.end), usage);
}

fn type_position(parent: Node<'_>, node: Node<'_>) -> bool {
    matches!(
        parent.kind(),
        "type_argument_list"
            | "union_case_list"
            | "type_parameter_constraint"
            | "nullable_type"
            | "array_type"
            | "tuple_element"
            | "pointer_type"
    ) || parent.kind() != "object_creation_expression"
        && ["returns", "type"]
            .iter()
            .any(|s| parent.child_by_field_name(s) == Some(node))
}
