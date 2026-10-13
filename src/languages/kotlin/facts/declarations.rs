use super::*;

pub(super) fn is_definition_node(node: Node<'_>) -> bool {
    matches!(
        node.kind(),
        "class_declaration"
            | "object_declaration"
            | "companion_object"
            | "function_declaration"
            | "secondary_constructor"
            | "property_declaration"
            | "type_alias"
            | "anonymous_function"
            | "lambda_literal"
            | "getter"
            | "setter"
            | "anonymous_initializer"
    )
}

pub(super) fn collect_definition(
    node: Node<'_>,
    context: &DefinitionContext<'_, '_>,
    sinks: &mut DefinitionSinks<'_>,
) {
    let source = context.source;
    let package = context.package;
    let contexts = context.contexts;
    let recovered_types = context.recovered_types;
    let kind = declaration_kind(node);
    let start_byte = crate::languages::kotlin::syntax::context_start(node, contexts);
    let owner = owner_path(node, source, recovered_types);
    let name_node = declaration_name_node(node);
    let name = name_node
        .map(|name| text(name, source).trim_matches('`').to_owned())
        .unwrap_or_else(|| generated_name(node));
    let name_range = name_node
        .map(|name| (name.start_byte(), name.end_byte()))
        .unwrap_or((node.start_byte(), node.end_byte()));
    if let Some(name_node) = name_node {
        record_declared_name(sinks, name_node);
    }
    let result = &mut *sinks.result;

    let modifier_node = named_children(node)
        .into_iter()
        .find(|child| child.kind() == "modifiers");
    let (visibility_level, modifier_flags, markers) = modifier_metadata(modifier_node, source);
    let scope = declaration_scope(node, context.root, recovered_types);
    let definition_kind = definition_kind(node, &owner);
    let is_interface_member = is_interface_member_declaration(node, recovered_types);
    let module = ModulePath(package.to_vec());

    result.definitions.push(Definition {
        name: name.clone(),
        kind: definition_kind,
        module,
        start_byte,
        end_byte: node.end_byte(),
        scope_start: scope.start,
        scope_end: scope.end,
        is_public: visibility_level == SemanticVisibility::Public,
        inline_module: false,
        external_module: false,
        context_id: 0,
    });

    let signature = callable_signature(node, source);
    let declared_type = declared_type(node, source);
    let supertypes = declaration_supertypes(node, source);
    let alias_target = (node.kind() == "type_alias")
        .then(|| alias_target(node, source))
        .flatten();
    // Keep raw annotations so the resolver can establish DslMarker metadata
    // across files, even when the annotation type is declared elsewhere.
    result.semantic.declarations.push(DeclarationFacts {
        start_byte,
        owner: owner.clone(),
        visibility: visibility_level,
        kind,
        declared_type: declared_type.clone(),
        supertypes,
        alias_target,
        signature,
        modifiers: modifier_flags,
        dsl_markers: markers.clone(),
        is_interface_member,
        execution: declaration_execution(node, context.scope_index),
    });

    capture_local_binding(
        node,
        source,
        &name,
        name_range,
        declared_type.as_deref(),
        result,
    );

    if let Some(receiver) = node.child_by_field_name("receiver") {
        let body = executable_body(node);
        result.semantic.receivers.push(ReceiverScope {
            start_byte: body.start,
            end_byte: body.end,
            receiver_type: normalized_type(text(receiver, source)),
            implicit: true,
            label: declaration_name(node, source),
            dsl_markers: Vec::new(),
        });
    }

    if matches!(
        node.kind(),
        "class_declaration" | "object_declaration" | "companion_object"
    ) && let Some(body) = class_body_scope(node)
    {
        let receiver_type = package
            .iter()
            .chain(&owner)
            .chain(std::iter::once(&name))
            .cloned()
            .collect::<Vec<_>>()
            .join(".");
        result.semantic.receivers.push(ReceiverScope {
            start_byte: body.start,
            end_byte: body.end,
            receiver_type,
            implicit: true,
            label: Some(name.clone()),
            dsl_markers: markers,
        });
    }
    capture_primary_constructor_parameters(node, context, sinks, &owner, &name);
}

pub(in crate::languages::kotlin::facts) fn package_name(
    nodes: &[Node<'_>],
    source: &[u8],
) -> Vec<String> {
    let Some(header) = nodes
        .iter()
        .copied()
        .find(|node| node.kind() == "package_header")
    else {
        return Vec::new();
    };
    named_children(header)
        .into_iter()
        .find(|node| node.kind() == "identifier")
        .map(|node| split_name(&text(node, source)))
        .unwrap_or_default()
}

pub(in crate::languages::kotlin::facts) fn split_name(value: &str) -> Vec<String> {
    value
        .split('.')
        .filter(|part| !part.is_empty())
        .map(|part| part.trim_matches('`').to_owned())
        .collect()
}

pub(in crate::languages::kotlin::facts) fn collect_imports(
    root: Node<'_>,
    nodes: &[Node<'_>],
    source: &[u8],
    package: &[String],
) -> Vec<Import> {
    let mut imports = Vec::new();
    for node in nodes
        .iter()
        .copied()
        .filter(|node| node.kind() == "import_header")
    {
        let path_node = named_children(node)
            .into_iter()
            .find(|child| child.kind() == "identifier");
        let Some(path_node) = path_node else {
            continue;
        };
        let path = split_name(&text(path_node, source));
        if path.is_empty() {
            continue;
        }
        let wildcard = named_children(node)
            .into_iter()
            .any(|child| child.kind() == "wildcard_import");
        let alias = named_children(node)
            .into_iter()
            .find(|child| child.kind() == "import_alias")
            .and_then(|alias| named_children(alias).into_iter().next())
            .map(|alias| text(alias, source).trim_matches('`').to_owned());
        let imported_name = (!wildcard).then(|| path.last().cloned()).flatten();
        imports.push(Import {
            path,
            alias,
            source: None,
            imported_name,
            namespace: wildcard,
            start_byte: node.start_byte(),
            end_byte: node.end_byte(),
            module: ModulePath(package.to_vec()),
            scope_start: root.start_byte(),
            scope_end: root.end_byte(),
            is_public: false,
            context_id: 0,
        });
    }
    imports
}

pub(super) fn capture_local_binding(
    node: Node<'_>,
    source: &[u8],
    name: &str,
    name_range: (usize, usize),
    declared_type: Option<&str>,
    result: &mut Capture,
) {
    let Some(outer_callable) =
        ancestors(node).find(|ancestor| is_executable_scope(ancestor.kind()))
    else {
        return;
    };
    let local_scope = ancestors(node)
        .take_while(|ancestor| ancestor.start_byte() >= outer_callable.start_byte())
        .find(|ancestor| ancestor.kind() == "statements")
        .map(range)
        .unwrap_or_else(|| executable_body(outer_callable));
    result.locals.push(LocalBinding {
        name: name.to_owned(),
        start_byte: name_range.0,
        end_byte: name_range.1,
        scope_start: local_scope.start,
        scope_end: local_scope.end,
        context_id: 0,
    });

    let (type_name, alias_target, callable_reference, immutable) =
        local_property_metadata(node, source, declared_type);
    result.semantic.bindings.push(BindingType {
        name: name.to_owned(),
        name_start_byte: name_range.0,
        name_end_byte: name_range.1,
        scope_start: local_scope.start,
        scope_end: local_scope.end,
        type_name,
        alias_target,
        callable_reference,
        immutable,
    });
}

pub(super) fn capture_primary_constructor_parameters(
    node: Node<'_>,
    context: &DefinitionContext<'_, '_>,
    sinks: &mut DefinitionSinks<'_>,
    owner: &[String],
    class_name: &str,
) {
    if !matches!(node.kind(), "class_declaration" | "object_declaration") {
        return;
    }
    let Some(parameters) = primary_constructor_parameters(node) else {
        return;
    };
    let source = context.source;
    let class_body = class_body_scope(node);
    for parameter in parameters {
        let Some(name_node) = named_children(parameter)
            .into_iter()
            .find(|child| child.kind() == "simple_identifier")
        else {
            continue;
        };
        let type_name = direct_type_children(parameter)
            .next()
            .map(|ty| normalized_type(text(ty, source)));
        let is_property = named_children(parameter)
            .into_iter()
            .any(|child| child.kind() == "binding_pattern_kind");
        if is_property {
            emit_constructor_property(
                node,
                parameter,
                context,
                sinks,
                owner
                    .iter()
                    .cloned()
                    .chain(std::iter::once(class_name.to_owned()))
                    .collect(),
                name_node,
                type_name,
            );
        } else if let Some(body) = class_body.clone() {
            push_binding(
                sinks.result,
                BindingEvidence {
                    name: text(name_node, source).trim_matches('`').to_owned(),
                    name_node,
                    scope: body,
                    type_name,
                    alias_target: None,
                    callable_reference: false,
                    immutable: true,
                },
            );
            record_declared_name(sinks, name_node);
        }
    }
}

pub(super) fn emit_constructor_property(
    class_node: Node<'_>,
    parameter: Node<'_>,
    context: &DefinitionContext<'_, '_>,
    sinks: &mut DefinitionSinks<'_>,
    owner: Vec<String>,
    name_node: Node<'_>,
    declared_type: Option<String>,
) {
    let source = context.source;
    let class_scope = class_body_scope(class_node).unwrap_or_else(|| range(class_node));
    let modifier_node = named_children(parameter)
        .into_iter()
        .find(|child| child.kind() == "modifiers");
    let (visibility, modifiers, _) = modifier_metadata(modifier_node, source);
    sinks.result.definitions.push(Definition {
        name: text(name_node, source).trim_matches('`').to_owned(),
        kind: DefinitionKind::Other,
        module: ModulePath(context.package.to_vec()),
        start_byte: parameter.start_byte(),
        end_byte: parameter.end_byte(),
        scope_start: class_scope.start,
        scope_end: class_scope.end,
        is_public: visibility == SemanticVisibility::Public,
        inline_module: false,
        external_module: false,
        context_id: 0,
    });
    sinks.result.semantic.declarations.push(DeclarationFacts {
        start_byte: parameter.start_byte(),
        owner,
        visibility,
        kind: DeclarationKind::Property,
        declared_type,
        supertypes: Vec::new(),
        alias_target: None,
        signature: None,
        modifiers,
        dsl_markers: Vec::new(),
        is_interface_member: false,
        execution: None,
    });
    record_declared_name(sinks, name_node);
}

pub(super) fn record_declared_name(sinks: &mut DefinitionSinks<'_>, name_node: Node<'_>) {
    sinks
        .declared_names
        .insert((name_node.start_byte(), name_node.end_byte()));
    sinks.excluded.push(name_node.byte_range());
}

pub(super) fn local_property_metadata(
    node: Node<'_>,
    source: &[u8],
    declared_type: Option<&str>,
) -> (Option<String>, Option<String>, bool, bool) {
    if node.kind() != "property_declaration" {
        return (None, None, false, true);
    }
    let Some(variable) = all_nodes(node)
        .into_iter()
        .find(|child| child.kind() == "variable_declaration")
    else {
        return (None, None, false, true);
    };
    let initializer = property_initializer(node, variable);
    let type_name = declared_type
        .map(str::to_owned)
        .or_else(|| initializer.and_then(|value| expression_type(value, source)));
    let (alias_target, callable_reference) = initializer
        .map(|initializer| initializer_alias(initializer, source))
        .unwrap_or_default();
    let immutable = !named_children(node)
        .into_iter()
        .find(|child| child.kind() == "binding_pattern_kind")
        .is_some_and(|kind| text(kind, source) == "var");
    (type_name, alias_target, callable_reference, immutable)
}

pub(super) fn declaration_kind(node: Node<'_>) -> DeclarationKind {
    match node.kind() {
        "function_declaration" | "anonymous_function" | "lambda_literal" => {
            DeclarationKind::Function
        }
        "secondary_constructor" => DeclarationKind::Constructor,
        "class_declaration" => DeclarationKind::Class,
        "object_declaration" => DeclarationKind::Object,
        "companion_object" => DeclarationKind::CompanionObject,
        "property_declaration" => DeclarationKind::Property,
        "type_alias" => DeclarationKind::TypeAlias,
        "getter" | "setter" => DeclarationKind::Accessor,
        _ => DeclarationKind::Other,
    }
}

pub(super) fn definition_kind(node: Node<'_>, owner: &[String]) -> DefinitionKind {
    match node.kind() {
        "class_declaration" => DefinitionKind::Struct,
        "type_alias" => DefinitionKind::TypeAlias,
        "function_declaration" | "anonymous_function" | "lambda_literal" => {
            if owner.is_empty() {
                DefinitionKind::Function
            } else {
                DefinitionKind::Method
            }
        }
        "secondary_constructor" | "getter" | "setter" | "anonymous_initializer" => {
            DefinitionKind::Method
        }
        _ => DefinitionKind::Other,
    }
}

pub(super) fn declaration_name_node(node: Node<'_>) -> Option<Node<'_>> {
    let wanted = match node.kind() {
        "class_declaration" | "object_declaration" | "type_alias" | "companion_object" => {
            return node.child_by_field_name("name").or_else(|| {
                named_children(node).into_iter().find(|child| {
                    matches!(
                        child.kind(),
                        "type_identifier" | "simple_identifier" | "identifier"
                    )
                })
            });
        }
        "function_declaration" | "property_declaration" => "simple_identifier",
        _ => return None,
    };
    if node.kind() == "property_declaration" {
        return all_nodes(node)
            .into_iter()
            .filter(|child| child.kind() == "variable_declaration")
            .flat_map(named_children)
            .find(|child| child.kind() == wanted);
    }
    named_children(node)
        .into_iter()
        .find(|child| child.kind() == wanted)
}

pub(super) fn declaration_name(node: Node<'_>, source: &[u8]) -> Option<String> {
    declaration_name_node(node)
        .map(|name| text(name, source).trim_matches('`').to_owned())
        .or_else(|| (node.kind() == "companion_object").then(|| "Companion".to_owned()))
}

pub(super) fn generated_name(node: Node<'_>) -> String {
    match node.kind() {
        "secondary_constructor" => "constructor".to_owned(),
        "companion_object" => "Companion".to_owned(),
        "getter" => "get".to_owned(),
        "setter" => "set".to_owned(),
        "anonymous_initializer" => "init".to_owned(),
        "anonymous_function" => "<anonymous-function>".to_owned(),
        "lambda_literal" => "<lambda>".to_owned(),
        _ => "<anonymous>".to_owned(),
    }
}

pub(super) fn owner_path(
    node: Node<'_>,
    source: &[u8],
    recovered_types: &[crate::languages::kotlin::syntax::RecoveredType],
) -> Vec<String> {
    owner_path_for(node, &node.byte_range(), None, source, recovered_types)
}

pub(super) fn owner_path_for(
    node: Node<'_>,
    declaration_range: &Range<usize>,
    excluded_recovered: Option<&Range<usize>>,
    source: &[u8],
    recovered_types: &[crate::languages::kotlin::syntax::RecoveredType],
) -> Vec<String> {
    let mut owners = node
        .parent()
        .into_iter()
        .flat_map(ancestors)
        .filter(|parent| {
            matches!(
                parent.kind(),
                "class_declaration" | "object_declaration" | "companion_object"
            )
        })
        .filter_map(|parent| {
            declaration_name(parent, source).map(|name| (parent.start_byte(), name))
        })
        .collect::<Vec<_>>();
    owners.extend(
        recovered_types
            .iter()
            .filter(|recovered| {
                Some(&recovered.declaration_range) != excluded_recovered
                    && recovered.body_range.start <= declaration_range.start
                    && declaration_range.end <= recovered.body_range.end
            })
            .map(|recovered| {
                let name = recovered.name_range.as_ref().map_or_else(
                    || "Companion".to_owned(),
                    |name_range| {
                        source
                            .get(name_range.clone())
                            .map(String::from_utf8_lossy)
                            .map(|name| name.trim_matches('`').to_owned())
                            .unwrap_or_default()
                    },
                );
                (recovered.declaration_range.start, name)
            }),
    );
    owners.sort_by_key(|(start, _)| *start);
    owners.dedup_by(|left, right| left.0 == right.0 && left.1 == right.1);
    owners.into_iter().map(|(_, name)| name).collect()
}

pub(super) fn nearest_type_declaration(node: Node<'_>) -> Option<Node<'_>> {
    ancestors(node)
        .take_while(|ancestor| !is_executable_scope(ancestor.kind()))
        .find(|parent| {
            matches!(
                parent.kind(),
                "class_declaration" | "object_declaration" | "companion_object"
            )
        })
}

pub(super) fn is_interface_member_declaration(
    node: Node<'_>,
    recovered_types: &[crate::languages::kotlin::syntax::RecoveredType],
) -> bool {
    if nearest_type_declaration(node).is_some_and(|owner| {
        (0..owner.child_count()).any(|index| {
            owner
                .child(index)
                .is_some_and(|child| child.kind() == "interface")
        })
    }) {
        return true;
    }
    let Some(recovered) =
        crate::languages::kotlin::syntax::recovered_type_for(node, recovered_types).filter(
            |recovered| {
                recovered.kind == crate::languages::kotlin::syntax::RecoveredTypeKind::Interface
            },
        )
    else {
        return false;
    };
    ancestors(node)
        .take_while(|ancestor| !is_executable_scope(ancestor.kind()))
        .any(|ancestor| {
            ancestor.kind() == "ERROR" && ancestor.byte_range() == recovered.error_range
        })
}

pub(super) fn declaration_scope(
    node: Node<'_>,
    root: Node<'_>,
    recovered_types: &[crate::languages::kotlin::syntax::RecoveredType],
) -> Range<usize> {
    if let Some(owner) = ancestors(node).find(|ancestor| is_executable_scope(ancestor.kind())) {
        return executable_body(owner);
    }
    if let Some(owner) = ancestors(node).find(|ancestor| {
        matches!(
            ancestor.kind(),
            "class_declaration" | "object_declaration" | "companion_object"
        )
    }) {
        return class_body_scope(owner).unwrap_or_else(|| range(owner));
    }
    if let Some(owner) = recovered_types
        .iter()
        .filter(|owner| {
            owner.body_range.start <= node.start_byte() && node.end_byte() <= owner.body_range.end
        })
        .min_by_key(|owner| owner.body_range.end - owner.body_range.start)
    {
        return owner.body_range.clone();
    }
    range(root)
}

pub(super) fn modifier_metadata(
    modifiers: Option<Node<'_>>,
    source: &[u8],
) -> (SemanticVisibility, DeclarationModifiers, Vec<String>) {
    let nodes = modifiers.map(all_nodes).unwrap_or_default();
    let words = nodes
        .iter()
        .copied()
        .filter(|child| child.child_count() == 0)
        .flat_map(|child| [text(child, source), child.kind().to_owned()])
        .collect::<HashSet<_>>();
    let contains = |word: &str| words.contains(word);
    let visibility = if contains("private") {
        SemanticVisibility::Private
    } else if contains("protected") {
        SemanticVisibility::Protected
    } else if contains("internal") {
        SemanticVisibility::Internal
    } else {
        SemanticVisibility::Public
    };
    let flags = DeclarationModifiers {
        is_expect: contains("expect"),
        is_actual: contains("actual"),
        is_abstract: contains("abstract"),
        is_open: contains("open"),
        is_final: contains("final"),
        is_override: contains("override"),
        is_operator: contains("operator"),
        is_infix: contains("infix"),
    };
    let annotations = nodes
        .into_iter()
        .filter(|child| child.kind() == "annotation")
        .flat_map(all_nodes)
        .filter(|child| child.kind() == "user_type")
        .map(|child| normalized_type(text(child, source)))
        .collect();
    (visibility, flags, annotations)
}
