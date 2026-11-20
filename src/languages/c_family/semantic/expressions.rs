use super::*;

pub(super) fn special(node: Node<'_>, source: &[u8], data: &mut Data, store: &mut SemanticStore) {
    match node.kind() {
        "namespace_alias_definition" => {
            if let Some(alias) = node.child_by_field_name("name")
                && let Some(target) = named_children(node).next_back()
            {
                data.aliases.insert(
                    text(alias, source),
                    (
                        node.start_byte(),
                        scope(node).end_byte(),
                        path(&text(target, source)),
                    ),
                );
            }
        }
        "pointer_expression"
            if node
                .child_by_field_name("operator")
                .is_some_and(|op| text(op, source) == "&") =>
        {
            if let Some(arg) = node
                .child_by_field_name("argument")
                .filter(|arg| matches!(arg.kind(), "identifier" | "qualified_identifier"))
                && binding_absent(arg, source, store)
            {
                calls::reference(
                    arg,
                    path(&text(arg, source)),
                    ReferenceKind::CallableReference,
                    source,
                    store,
                );
            }
        }
        "init_declarator" => {
            let Some(args) = node
                .child_by_field_name("value")
                .filter(|v| matches!(v.kind(), "argument_list" | "initializer_list"))
            else {
                return;
            };
            let Some(ty) = node
                .parent()
                .and_then(|declaration| declaration.child_by_field_name("type"))
                .filter(|ty| {
                    matches!(
                        ty.kind(),
                        "type_identifier" | "qualified_identifier" | "template_type"
                    )
                })
            else {
                return;
            };
            let arguments = named_children(args)
                .filter(|item| item.kind() != "comment")
                .map(|arg| calls::type_of(arg, source, store))
                .collect();
            calls::reference(
                args,
                path(&text(ty, source)),
                ReferenceKind::Call,
                source,
                store,
            );
            data.calls.insert(
                args.start_byte(),
                Invocation {
                    arguments,
                    constructor: true,
                    ..Default::default()
                },
            );
        }
        "binary_expression" => {
            let Some((left, right)) = node
                .child_by_field_name("left")
                .zip(node.child_by_field_name("right"))
            else {
                return;
            };
            let Some(ty) = calls::type_of(left, source, store) else {
                return;
            };
            if !class_type(&ty) {
                return;
            }
            let Some(op) = node.child_by_field_name("operator") else {
                return;
            };
            let name = format!("operator{}", text(op, source));
            calls::reference(op, vec![name], ReferenceKind::Call, source, store);
            let arguments = vec![Some(ty), calls::type_of(right, source, store)];
            data.calls.insert(
                op.start_byte(),
                Invocation {
                    arguments,
                    operator: true,
                    ..Default::default()
                },
            );
        }
        _ => {}
    }
}

fn binding_absent(node: Node<'_>, source: &[u8], store: &SemanticStore) -> bool {
    calls::binding(store, &text(node, source), node.start_byte()).is_none()
}

pub(super) fn class_type(name: &str) -> bool {
    let base = name
        .trim_start_matches("const ")
        .trim_end_matches([' ', '&', '*']);
    !base.is_empty()
        && !base.starts_with("unsigned")
        && !base.starts_with("signed")
        && !matches!(
            base,
            "auto"
                | "bool"
                | "char"
                | "short"
                | "int"
                | "long"
                | "long long"
                | "float"
                | "double"
                | "long double"
                | "void"
        )
}
