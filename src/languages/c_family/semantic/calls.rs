use super::*;

pub(super) fn call(node: Node<'_>, source: &[u8], data: &mut Data, store: &mut SemanticStore) {
    let Some(target) = node.child_by_field_name("function") else {
        return;
    };
    let mut function = target;
    while function.kind() == "parenthesized_expression" {
        let Some(child) = function.named_child(0) else {
            return;
        };
        function = child;
    }
    let receiver = function
        .child_by_field_name("argument")
        .map(|item| text(item, source));
    let member = function.child_by_field_name("field").unwrap_or(function);
    let name = member
        .child_by_field_name("name")
        .filter(|_| matches!(member.kind(), "template_method" | "template_function"))
        .unwrap_or(member);
    let mut spelling = text(name, source);
    let mut receiver = receiver;
    if function.kind() == "identifier"
        && let Some(local) = binding(store, &spelling, function.start_byte())
        && !local.callable_reference
        && local
            .type_name
            .as_deref()
            .is_some_and(expressions::class_type)
    {
        receiver = Some(std::mem::replace(&mut spelling, "operator()".into()));
    }
    let args = node.child_by_field_name("arguments");
    let arguments: Vec<_> = args
        .into_iter()
        .flat_map(named_children)
        .filter(|arg| arg.kind() != "comment")
        .map(|arg| type_of(arg, source, store))
        .collect();
    let indirect = dependent(node, source, &arguments)
        || !matches!(
            function.kind(),
            "identifier" | "qualified_identifier" | "field_expression" | "template_function"
        );
    let qualified = spelling.starts_with("::")
        || spelling.contains("::")
        || member.kind() == "qualified_identifier";
    let call = Invocation {
        arguments,
        receiver,
        indirect,
        qualified,
        ..Default::default()
    };
    let kind = if call.receiver.is_some() {
        ReferenceKind::Method
    } else {
        ReferenceKind::Call
    };
    reference(target, path(&spelling), kind, source, store);
    data.calls.insert(target.start_byte(), call);
}

pub(super) fn type_reference(
    node: Node<'_>,
    source: &[u8],
    data: &mut Data,
    store: &mut SemanticStore,
) {
    let Some(parent) = node.parent() else { return };
    if parent.child_by_field_name("name") == Some(node)
        && matches!(
            parent.kind(),
            "struct_specifier"
                | "class_specifier"
                | "enum_specifier"
                | "union_specifier"
                | "alias_declaration"
        )
        && (parent.child_by_field_name("body").is_some() || parent.kind() == "alias_declaration")
    {
        return;
    }
    if parent.child_by_field_name("declarator") == Some(node) && parent.kind() == "type_definition"
    {
        return;
    }
    let target = if parent.kind() == "qualified_identifier" {
        parent
    } else {
        node
    };
    if let Some(tag) = match parent.kind() {
        "struct_specifier" | "class_specifier" => Some(DefinitionKind::Struct),
        "enum_specifier" => Some(DefinitionKind::Enum),
        "union_specifier" => Some(DefinitionKind::Union),
        _ => None,
    } {
        data.tags.insert(target.start_byte(), tag);
    }
    reference(
        target,
        path(&text(target, source)),
        ReferenceKind::Type,
        source,
        store,
    );
}

pub(super) fn reference(
    node: Node<'_>,
    path: Vec<String>,
    kind: ReferenceKind,
    source: &[u8],
    store: &mut SemanticStore,
) {
    let owner = ancestors(node)
        .find(|parent| matches!(parent.kind(), "function_definition" | "lambda_expression"))
        .map(|parent| parent.start_byte());
    let scope = scope(node);
    store.references.push(Reference {
        path,
        module: ModulePath(owner_path(node, source)),
        kind,
        start_byte: node.start_byte(),
        end_byte: node.end_byte(),
        scope_start: scope.start_byte(),
        scope_end: scope.end_byte(),
        call_owner: owner,
        execution_owner: owner,
        access: None,
        context_id: 0,
    });
}

pub(super) fn type_of(node: Node<'_>, source: &[u8], store: &SemanticStore) -> Option<String> {
    let literal = text(node, source);
    match node.kind() {
        "true" | "false" => Some("bool".into()),
        "char_literal" => Some("char".into()),
        "string_literal" | "raw_string_literal" => Some("const char*".into()),
        "number_literal" => {
            let value = literal.to_ascii_lowercase();
            let floating = value.contains('.')
                || if value.starts_with("0x") {
                    value.contains('p')
                } else {
                    value.contains('e')
                };
            Some(
                if floating && value.ends_with('f') {
                    "float"
                } else if floating && value.ends_with('l') {
                    "long double"
                } else if floating {
                    "double"
                } else if value.ends_with("ull") {
                    "unsigned long long"
                } else if value.ends_with("ll") {
                    "long long"
                } else if value.ends_with("ul") {
                    "unsigned long"
                } else if value.ends_with('l') {
                    "long"
                } else if value.ends_with('u') {
                    "unsigned int"
                } else {
                    "int"
                }
                .into(),
            )
        }
        "identifier" => {
            binding(store, &literal, node.start_byte()).and_then(|local| local.type_name.clone())
        }
        "cast_expression" => node.child_by_field_name("type").map(|ty| text(ty, source)),
        _ => None,
    }
}

pub(crate) fn binding<'a>(
    store: &'a SemanticStore,
    name: &str,
    at: usize,
) -> Option<&'a LocalBinding> {
    store
        .locals
        .iter()
        .filter(|local| {
            local.name == name
                && local.start_byte <= at
                && local.scope_start <= at
                && at < local.scope_end
        })
        .max_by_key(|local| (local.scope_start, local.start_byte))
}

fn dependent(node: Node<'_>, source: &[u8], arguments: &[Option<String>]) -> bool {
    let parameters = ancestors(node)
        .filter(|parent| parent.kind() == "template_declaration")
        .filter_map(|template| template.child_by_field_name("parameters"))
        .flat_map(named_children)
        .filter(|parameter| parameter.kind().contains("type_parameter_declaration"))
        .filter_map(|parameter| {
            named_children(parameter).find(|child| child.kind() == "type_identifier")
        });
    parameters.map(|name| text(name, source)).any(|name| {
        arguments.iter().flatten().any(|arg| {
            arg.split(|ch: char| !(ch.is_alphanumeric() || ch == '_'))
                .any(|word| word == name)
        })
    })
}
