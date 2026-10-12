//! Kotlin-specific syntax facts for the shared resolver and call graph.

use std::collections::HashSet;
use std::ops::Range;

use tree_sitter::Node;

use crate::graph::{
    BindingType, CallArgumentFact, CallFacts, CallableParameter, CallableSignature,
    CallableTypeParameter, DeclarationExecutionFacts, DeclarationFacts, DeclarationKind,
    DeclarationModifiers, ReceiverScope, SemanticContext, SemanticFacts, SemanticVisibility,
};
use crate::model::{
    Definition, DefinitionKind, Import, LocalBinding, ModulePath, Reference, ReferenceAccess,
    ReferenceKind, SourceRange,
};

#[derive(Default)]
pub(super) struct Capture {
    pub definitions: Vec<Definition>,
    pub imports: Vec<Import>,
    pub references: Vec<Reference>,
    pub locals: Vec<LocalBinding>,
    pub semantic: SemanticFacts,
}

struct DefinitionContext<'tree, 'source> {
    root: Node<'tree>,
    source: &'source [u8],
    package: &'source [String],
    contexts: &'source [super::syntax::ContextParameters],
    recovered_types: &'source [super::syntax::RecoveredType],
}

struct DefinitionSinks<'capture> {
    result: &'capture mut Capture,
    declarations: &'capture mut Vec<DeclarationFacts>,
    excluded: &'capture mut Vec<Range<usize>>,
    declared_names: &'capture mut HashSet<(usize, usize)>,
}

pub(super) fn capture(
    root: Node<'_>,
    source: &[u8],
    contexts: &[super::syntax::ContextParameters],
) -> Capture {
    let nodes = all_nodes(root);
    let recovered_types = super::syntax::recovered_types(root, source);
    let package = package_name(root, source);
    let mut result = Capture::default();
    result.semantic.context = Some(SemanticContext {
        package: package.clone(),
    });

    let imports = collect_imports(root, source, &package);
    let mut excluded = imports
        .iter()
        .map(|import| import.start_byte..import.end_byte)
        .collect::<Vec<_>>();
    let mut declarations = Vec::new();
    let mut declared_names = HashSet::new();
    let mut dsl_markers = HashSet::new();
    let definition_context = DefinitionContext {
        root,
        source,
        package: &package,
        contexts,
        recovered_types: &recovered_types,
    };

    for node in nodes.iter().copied() {
        if node.kind() == "class_declaration"
            && has_annotation(node, source, "DslMarker")
            && let Some(name) = declaration_name(node, source)
        {
            dsl_markers.insert(name);
        }
    }

    for node in nodes.iter().copied() {
        if is_definition_node(node) {
            collect_definition(
                node,
                &definition_context,
                &mut DefinitionSinks {
                    result: &mut result,
                    declarations: &mut declarations,
                    excluded: &mut excluded,
                    declared_names: &mut declared_names,
                },
            );
        }
    }

    collect_recovered_type_definitions(
        root,
        source,
        &package,
        contexts,
        &recovered_types,
        &mut result,
        &mut declarations,
    );

    result.imports = imports;
    result.semantic.declarations = declarations;
    collect_locals(root, source, &nodes, &declared_names, &mut result);
    collect_receivers(root, source, &nodes, &dsl_markers, &mut result.semantic);
    collect_context_facts(root, source, &nodes, contexts, &mut result);
    result.semantic.calls = collect_calls(&nodes, source);
    enrich_call_argument_types(
        &mut result.semantic.calls,
        &result.semantic.bindings,
        &nodes,
        source,
    );
    result.references = collect_references(root, source, &nodes, &excluded, contexts);
    let conventions = super::conventions::capture(
        root,
        source,
        contexts,
        &result.semantic.bindings,
        &result.semantic.receivers,
    );
    let convention_ranges = conventions
        .references
        .iter()
        .map(|reference| (reference.start_byte, reference.end_byte))
        .collect::<HashSet<_>>();
    result.references.retain(|reference| {
        !convention_ranges.contains(&(reference.start_byte, reference.end_byte))
    });
    result.references.extend(conventions.references);
    merge_convention_calls(&mut result.semantic.calls, conventions.calls);
    result
}

fn merge_convention_calls(calls: &mut Vec<CallFacts>, conventions: Vec<CallFacts>) {
    for convention in conventions {
        let Some(existing) = calls.iter_mut().find(|call| {
            call.start_byte == convention.start_byte && call.end_byte == convention.end_byte
        }) else {
            calls.push(convention);
            continue;
        };
        existing.convention = convention.convention.or(existing.convention);
        existing.target = convention.target.or(existing.target);
        existing.receiver_type = convention
            .receiver_type
            .or_else(|| existing.receiver_type.clone());
        for argument in convention.arguments {
            if let Some(existing_argument) = existing.arguments.iter_mut().find(|existing| {
                existing.start_byte == argument.start_byte && existing.end_byte == argument.end_byte
            }) {
                existing_argument.type_name = argument
                    .type_name
                    .or_else(|| existing_argument.type_name.clone());
                existing_argument.is_lambda |= argument.is_lambda;
                if existing_argument.lambda_parameter_types.is_empty() {
                    existing_argument.lambda_parameter_types = argument.lambda_parameter_types;
                }
                existing_argument.lambda_receiver_type = argument
                    .lambda_receiver_type
                    .or_else(|| existing_argument.lambda_receiver_type.clone());
            } else {
                existing.arguments.push(argument);
            }
        }
    }
    calls.sort_by_key(|call| (call.start_byte, call.end_byte));
}

fn named_children<'tree>(node: Node<'tree>) -> Vec<Node<'tree>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
}

fn all_nodes<'tree>(root: Node<'tree>) -> Vec<Node<'tree>> {
    let mut result = Vec::new();
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        result.push(node);
        let mut cursor = node.walk();
        pending.extend(node.children(&mut cursor));
    }
    result.sort_by_key(|node| (node.start_byte(), node.end_byte()));
    result
}

fn text(node: Node<'_>, source: &[u8]) -> String {
    source
        .get(node.byte_range())
        .map(String::from_utf8_lossy)
        .map(|value| value.trim().to_owned())
        .unwrap_or_default()
}

fn package_name(root: Node<'_>, source: &[u8]) -> Vec<String> {
    let Some(header) = all_nodes(root)
        .into_iter()
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

fn split_name(value: &str) -> Vec<String> {
    value
        .split('.')
        .filter(|part| !part.is_empty())
        .map(|part| part.trim_matches('`').to_owned())
        .collect()
}

fn collect_imports(root: Node<'_>, source: &[u8], package: &[String]) -> Vec<Import> {
    let mut imports = Vec::new();
    for node in all_nodes(root)
        .into_iter()
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

fn is_definition_node(node: Node<'_>) -> bool {
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

fn collect_definition(
    node: Node<'_>,
    context: &DefinitionContext<'_, '_>,
    sinks: &mut DefinitionSinks<'_>,
) {
    let DefinitionContext {
        root,
        source,
        package,
        contexts,
        recovered_types,
    } = context;
    let root = *root;
    let source = *source;
    let package = *package;
    let contexts = *contexts;
    let recovered_types = *recovered_types;
    let DefinitionSinks {
        result,
        declarations,
        excluded,
        declared_names,
    } = sinks;
    let result = &mut **result;
    let declarations = &mut **declarations;
    let excluded = &mut **excluded;
    let declared_names = &mut **declared_names;

    let kind = declaration_kind(node);
    let start_byte = super::syntax::context_start(node, contexts);
    let owner = owner_path(node, source, recovered_types);
    let mut name_node = declaration_name_node(node);
    let name = name_node
        .map(|name| text(name, source).trim_matches('`').to_owned())
        .unwrap_or_else(|| generated_name(node));
    let name_range = name_node
        .map(|name| (name.start_byte(), name.end_byte()))
        .unwrap_or((node.start_byte(), node.end_byte()));
    if let Some(name_node) = name_node.take() {
        excluded.push(name_node.byte_range());
        declared_names.insert((name_node.start_byte(), name_node.end_byte()));
    }

    let visibility_level = visibility(node, source);
    let modifier_flags = modifiers(node, source);
    let scope = declaration_scope(node, root, recovered_types);
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
    let markers = annotation_names(node, source);
    declarations.push(DeclarationFacts {
        start_byte,
        owner: owner.clone(),
        visibility: visibility_level,
        kind,
        declared_type: declared_type.clone(),
        supertypes,
        alias_target: alias_target.clone(),
        signature: signature.clone(),
        modifiers: modifier_flags,
        dsl_markers: markers.clone(),
        is_interface_member,
        execution: declaration_execution(node, root, source, contexts),
    });

    if let Some(outer_callable) =
        ancestors(node).find(|ancestor| is_executable_scope(ancestor.kind()))
    {
        let local_scope = ancestors(node)
            .take_while(|ancestor| ancestor.start_byte() >= outer_callable.start_byte())
            .find(|ancestor| ancestor.kind() == "statements")
            .map(range)
            .or_else(|| executable_body(outer_callable))
            .unwrap_or_else(|| range(outer_callable));
        result.locals.push(LocalBinding {
            name: name.clone(),
            start_byte: name_range.0,
            end_byte: name_range.1,
            scope_start: local_scope.start,
            scope_end: local_scope.end,
            context_id: 0,
        });
        let local_property = (node.kind() == "property_declaration")
            .then(|| {
                let variable = all_nodes(node)
                    .into_iter()
                    .find(|child| child.kind() == "variable_declaration")?;
                let initializer = property_initializer(node, variable);
                let type_name = declared_type
                    .clone()
                    .or_else(|| initializer.and_then(|value| expression_type(value, source)));
                let callable_reference = initializer
                    .is_some_and(|initializer| initializer.kind() == "callable_reference");
                let alias_target = initializer.and_then(|initializer| {
                    if callable_reference {
                        reference_target(initializer, source).map(|(path, _)| path.join("."))
                    } else if matches!(
                        initializer.kind(),
                        "simple_identifier" | "navigation_expression"
                    ) {
                        simple_path(initializer, source).map(|path| path.join("."))
                    } else {
                        None
                    }
                });
                let immutable = !named_children(node)
                    .into_iter()
                    .find(|child| child.kind() == "binding_pattern_kind")
                    .is_some_and(|kind| text(kind, source) == "var");
                Some((type_name, alias_target, callable_reference, immutable))
            })
            .flatten();
        let (type_name, alias_target, callable_reference, immutable) =
            local_property.unwrap_or((None, None, false, true));
        result.semantic.bindings.push(BindingType {
            name: name.clone(),
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

    if (node.kind() == "class_declaration" || node.kind() == "object_declaration")
        && let Some(parameters) = primary_constructor_parameters(node)
    {
        for parameter in parameters {
            let Some(name_node) = named_children(parameter)
                .into_iter()
                .find(|child| child.kind() == "simple_identifier")
            else {
                continue;
            };
            let parameter_name = text(name_node, source).trim_matches('`').to_owned();
            let has_property = named_children(parameter)
                .into_iter()
                .any(|child| child.kind() == "binding_pattern_kind");
            let type_name = direct_type_children(parameter)
                .into_iter()
                .next()
                .map(|ty| normalized_type(text(ty, source)));
            if has_property {
                let property_kind = DefinitionKind::Other;
                let class_scope = class_body_scope(node).unwrap_or_else(|| range(node));
                result.definitions.push(Definition {
                    name: parameter_name.clone(),
                    kind: property_kind,
                    module: ModulePath(package.to_vec()),
                    start_byte: parameter.start_byte(),
                    end_byte: parameter.end_byte(),
                    scope_start: class_scope.start,
                    scope_end: class_scope.end,
                    is_public: visibility(parameter, source) == SemanticVisibility::Public,
                    inline_module: false,
                    external_module: false,
                    context_id: 0,
                });
                declarations.push(DeclarationFacts {
                    start_byte: parameter.start_byte(),
                    owner: owner
                        .iter()
                        .cloned()
                        .chain(std::iter::once(name.clone()))
                        .collect(),
                    visibility: visibility(parameter, source),
                    kind: DeclarationKind::Property,
                    declared_type: type_name.clone(),
                    supertypes: Vec::new(),
                    alias_target: None,
                    signature: None,
                    modifiers: modifiers(parameter, source),
                    dsl_markers: Vec::new(),
                    is_interface_member: false,
                    execution: None,
                });
                declared_names.insert((name_node.start_byte(), name_node.end_byte()));
                excluded.push(name_node.byte_range());
            } else if let Some(body) = class_body_scope(node) {
                push_binding(
                    result,
                    BindingEvidence {
                        name: parameter_name.clone(),
                        name_node,
                        scope: body,
                        type_name,
                        alias_target: None,
                        callable_reference: false,
                        immutable: true,
                    },
                );
                declared_names.insert((name_node.start_byte(), name_node.end_byte()));
                excluded.push(name_node.byte_range());
            }
        }
    }

    if let Some(receiver) = node.child_by_field_name("receiver")
        && let Some(body) = executable_body(node)
    {
        result.semantic.receivers.push(ReceiverScope {
            start_byte: body.start,
            end_byte: body.end,
            receiver_type: normalized_type(text(receiver, source)),
            implicit: true,
            label: declaration_name(node, source),
            dsl_markers: Vec::new(),
        });
    }

    if (node.kind() == "class_declaration"
        || node.kind() == "object_declaration"
        || node.kind() == "companion_object")
        && let Some(body) = class_body_scope(node)
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
}

fn collect_recovered_type_definitions(
    root: Node<'_>,
    source: &[u8],
    package: &[String],
    contexts: &[super::syntax::ContextParameters],
    recovered_types: &[super::syntax::RecoveredType],
    result: &mut Capture,
    declarations: &mut Vec<DeclarationFacts>,
) {
    let nodes = all_nodes(root);
    for recovered in recovered_types {
        let Some(error) = nodes
            .iter()
            .copied()
            .find(|node| node.kind() == "ERROR" && node.byte_range() == recovered.error_range)
        else {
            continue;
        };
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
        if name.is_empty() {
            continue;
        }
        let start_byte = recovered.declaration_range.start;
        let owner = recovered_owner_path(error, recovered, source, recovered_types);
        let kind = match recovered.kind {
            super::syntax::RecoveredTypeKind::Class
            | super::syntax::RecoveredTypeKind::Interface => DeclarationKind::Class,
            super::syntax::RecoveredTypeKind::Object => DeclarationKind::Object,
            super::syntax::RecoveredTypeKind::CompanionObject => DeclarationKind::CompanionObject,
        };
        let visibility = recovered_visibility(error, recovered, source);
        let modifier_flags = recovered_modifiers(error, recovered, source);
        let scope = declaration_scope(error, root, recovered_types);
        let definition_kind = match recovered.kind {
            super::syntax::RecoveredTypeKind::Class => DefinitionKind::Struct,
            super::syntax::RecoveredTypeKind::Interface => DefinitionKind::Trait,
            super::syntax::RecoveredTypeKind::Object => DefinitionKind::Struct,
            super::syntax::RecoveredTypeKind::CompanionObject => DefinitionKind::Other,
        };
        result.definitions.push(Definition {
            name: name.clone(),
            kind: definition_kind,
            module: ModulePath(package.to_vec()),
            start_byte,
            end_byte: recovered.declaration_range.end,
            scope_start: scope.start,
            scope_end: scope.end,
            is_public: visibility == SemanticVisibility::Public,
            inline_module: false,
            external_module: false,
            context_id: 0,
        });

        if let Some(outer_callable) =
            ancestors(error).find(|ancestor| is_executable_scope(ancestor.kind()))
        {
            let local_scope =
                executable_body(outer_callable).unwrap_or_else(|| range(outer_callable));
            result.locals.push(LocalBinding {
                name: name.clone(),
                start_byte: recovered
                    .name_range
                    .as_ref()
                    .map_or(start_byte, |range| range.start),
                end_byte: recovered
                    .name_range
                    .as_ref()
                    .map_or(start_byte, |range| range.end),
                scope_start: local_scope.start,
                scope_end: local_scope.end,
                context_id: 0,
            });
        }

        let signature = recovered_signature(error, recovered, source);
        declarations.push(DeclarationFacts {
            start_byte,
            owner,
            visibility,
            kind,
            declared_type: Some(name),
            supertypes: recovered_supertypes(error, recovered, source),
            alias_target: None,
            signature,
            modifiers: modifier_flags,
            is_interface_member: false,
            dsl_markers: recovered_annotations(error, recovered, source),
            execution: recovered_type_execution(recovered, root, source, contexts, recovered_types),
        });
    }
}

fn recovered_owner_path(
    error: Node<'_>,
    declaration: &super::syntax::RecoveredType,
    source: &[u8],
    recovered_types: &[super::syntax::RecoveredType],
) -> Vec<String> {
    let mut owners = error
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
            .filter(|owner| {
                owner.declaration_range != declaration.declaration_range
                    && owner.body_range.start <= declaration.declaration_range.start
                    && declaration.declaration_range.end <= owner.body_range.end
            })
            .map(|owner| {
                let name = owner.name_range.as_ref().map_or_else(
                    || "Companion".to_owned(),
                    |name_range| {
                        source
                            .get(name_range.clone())
                            .map(String::from_utf8_lossy)
                            .map(|name| name.trim_matches('`').to_owned())
                            .unwrap_or_default()
                    },
                );
                (owner.declaration_range.start, name)
            }),
    );
    owners.sort_by_key(|(start, _)| *start);
    owners.dedup_by(|left, right| left.0 == right.0 && left.1 == right.1);
    owners.into_iter().map(|(_, name)| name).collect()
}

fn recovered_header_children<'tree>(
    error: Node<'tree>,
    recovered: &super::syntax::RecoveredType,
) -> Vec<Node<'tree>> {
    named_children(error)
        .into_iter()
        .filter(|child| {
            child.start_byte() >= recovered.keyword_range.end
                && child.end_byte() <= recovered.body_range.start
        })
        .collect()
}

fn recovered_supertypes(
    error: Node<'_>,
    recovered: &super::syntax::RecoveredType,
    source: &[u8],
) -> Vec<String> {
    let mut result = Vec::new();
    for child in recovered_header_children(error, recovered) {
        if child.kind() == "delegation_specifier" {
            if let Some(user_type) = all_nodes(child)
                .into_iter()
                .find(|node| node.kind() == "user_type")
            {
                result.push(normalized_type(text(user_type, source)));
            }
        } else if child.kind() == "delegation_specifiers" {
            result.extend(
                named_children(child)
                    .into_iter()
                    .filter(|specifier| specifier.kind() == "delegation_specifier")
                    .filter_map(|specifier| {
                        all_nodes(specifier)
                            .into_iter()
                            .find(|node| node.kind() == "user_type")
                    })
                    .map(|user_type| normalized_type(text(user_type, source))),
            );
        }
    }
    result
}

fn recovered_signature(
    error: Node<'_>,
    recovered: &super::syntax::RecoveredType,
    source: &[u8],
) -> Option<CallableSignature> {
    if !matches!(
        recovered.kind,
        super::syntax::RecoveredTypeKind::Class
            | super::syntax::RecoveredTypeKind::Interface
            | super::syntax::RecoveredTypeKind::Object
    ) {
        return None;
    }
    let header = recovered_header_children(error, recovered);
    let parameters = if recovered.kind == super::syntax::RecoveredTypeKind::Interface {
        Vec::new()
    } else {
        header
            .iter()
            .copied()
            .find(|child| child.kind() == "primary_constructor")
            .into_iter()
            .flat_map(all_nodes)
            .filter(|child| child.kind() == "class_parameter")
            .filter_map(|parameter| parameter_info(parameter, source))
            .collect()
    };
    let mut type_parameters = Vec::<CallableTypeParameter>::new();
    for parameter in header
        .iter()
        .copied()
        .filter(|child| child.kind() == "type_parameters")
        .flat_map(named_children)
        .filter(|child| child.kind() == "type_parameter")
    {
        let Some(name) = named_children(parameter)
            .into_iter()
            .find(|child| child.kind() == "type_identifier")
            .map(|name| text(name, source).trim_matches('`').to_owned())
        else {
            continue;
        };
        let mut bounds = direct_type_children(parameter)
            .into_iter()
            .map(|bound| normalized_type(text(bound, source)))
            .collect::<Vec<_>>();
        normalize_bounds(&mut bounds);
        type_parameters.push(CallableTypeParameter { name, bounds });
    }
    for constraint in header
        .iter()
        .copied()
        .filter(|child| child.kind() == "type_constraints")
        .flat_map(named_children)
        .filter(|child| child.kind() == "type_constraint")
    {
        let Some(name) = named_children(constraint)
            .into_iter()
            .find(|child| child.kind() == "type_identifier")
            .map(|name| text(name, source).trim_matches('`').to_owned())
        else {
            continue;
        };
        if let Some(parameter) = type_parameters.iter_mut().find(|item| item.name == name) {
            parameter.bounds.extend(
                direct_type_children(constraint)
                    .into_iter()
                    .map(|bound| normalized_type(text(bound, source))),
            );
            normalize_bounds(&mut parameter.bounds);
        }
    }
    Some(CallableSignature {
        type_parameters,
        parameters,
        return_type: None,
        receiver_type: None,
    })
}

fn recovered_modifier_node<'tree>(
    error: Node<'tree>,
    recovered: &super::syntax::RecoveredType,
) -> Option<Node<'tree>> {
    let mut before = (0..error.child_count())
        .filter_map(|index| error.child(index))
        .filter(|child| child.end_byte() <= recovered.keyword_range.start)
        .collect::<Vec<_>>();
    before.reverse();
    for child in before {
        if matches!(child.kind(), "line_comment" | "multiline_comment") {
            continue;
        }
        return (child.kind() == "modifiers").then_some(child);
    }
    None
}

fn recovered_has_modifier(
    error: Node<'_>,
    recovered: &super::syntax::RecoveredType,
    source: &[u8],
    name: &str,
) -> bool {
    recovered_modifier_node(error, recovered).is_some_and(|modifiers| {
        all_nodes(modifiers)
            .into_iter()
            .filter(|child| child.child_count() == 0)
            .any(|child| text(child, source) == name || child.kind() == name)
    })
}

fn recovered_visibility(
    error: Node<'_>,
    recovered: &super::syntax::RecoveredType,
    source: &[u8],
) -> SemanticVisibility {
    if recovered_has_modifier(error, recovered, source, "private") {
        SemanticVisibility::Private
    } else if recovered_has_modifier(error, recovered, source, "protected") {
        SemanticVisibility::Protected
    } else if recovered_has_modifier(error, recovered, source, "internal") {
        SemanticVisibility::Internal
    } else {
        SemanticVisibility::Public
    }
}

fn recovered_modifiers(
    error: Node<'_>,
    recovered: &super::syntax::RecoveredType,
    source: &[u8],
) -> DeclarationModifiers {
    let has = |name| recovered_has_modifier(error, recovered, source, name);
    DeclarationModifiers {
        is_expect: has("expect"),
        is_actual: has("actual"),
        is_abstract: has("abstract"),
        is_open: has("open"),
        is_final: has("final"),
        is_override: has("override"),
        is_operator: false,
        is_infix: false,
    }
}

fn recovered_annotations(
    error: Node<'_>,
    recovered: &super::syntax::RecoveredType,
    source: &[u8],
) -> Vec<String> {
    recovered_modifier_node(error, recovered)
        .into_iter()
        .flat_map(all_nodes)
        .filter(|child| child.kind() == "annotation")
        .flat_map(all_nodes)
        .filter(|child| child.kind() == "user_type")
        .map(|child| normalized_type(text(child, source)))
        .collect()
}

fn declaration_kind(node: Node<'_>) -> DeclarationKind {
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

fn definition_kind(node: Node<'_>, owner: &[String]) -> DefinitionKind {
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
        "property_declaration" => DefinitionKind::Other,
        _ => DefinitionKind::Other,
    }
}

fn declaration_name_node(node: Node<'_>) -> Option<Node<'_>> {
    let wanted = match node.kind() {
        "class_declaration" | "object_declaration" | "type_alias" => {
            return node.child_by_field_name("name").or_else(|| {
                named_children(node).into_iter().find(|child| {
                    matches!(
                        child.kind(),
                        "type_identifier" | "simple_identifier" | "identifier"
                    )
                })
            });
        }
        "companion_object" => {
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
        "secondary_constructor" => return None,
        _ => return None,
    };
    if node.kind() == "property_declaration" {
        return all_nodes(node)
            .into_iter()
            .filter(|child| child.kind() == "variable_declaration")
            .flat_map(|child| named_children(child).into_iter().collect::<Vec<_>>())
            .find(|child| child.kind() == wanted);
    }
    named_children(node)
        .into_iter()
        .find(|child| child.kind() == wanted)
}

fn declaration_name(node: Node<'_>, source: &[u8]) -> Option<String> {
    declaration_name_node(node)
        .map(|name| text(name, source).trim_matches('`').to_owned())
        .or_else(|| (node.kind() == "companion_object").then(|| "Companion".to_owned()))
}

fn generated_name(node: Node<'_>) -> String {
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

fn owner_path(
    node: Node<'_>,
    source: &[u8],
    recovered_types: &[super::syntax::RecoveredType],
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
                recovered.body_range.start <= node.start_byte()
                    && node.end_byte() <= recovered.body_range.end
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

fn ancestors<'tree>(mut node: Node<'tree>) -> impl Iterator<Item = Node<'tree>> {
    std::iter::from_fn(move || {
        let current = node;
        node = current.parent()?;
        Some(node)
    })
}

fn nearest_type_declaration(node: Node<'_>) -> Option<Node<'_>> {
    ancestors(node)
        .take_while(|ancestor| !is_executable_scope(ancestor.kind()))
        .find(|parent| {
            matches!(
                parent.kind(),
                "class_declaration" | "object_declaration" | "companion_object"
            )
        })
}

fn is_interface_member_declaration(
    node: Node<'_>,
    recovered_types: &[super::syntax::RecoveredType],
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
    let Some(recovered) = super::syntax::recovered_type_for(node, recovered_types)
        .filter(|recovered| recovered.kind == super::syntax::RecoveredTypeKind::Interface)
    else {
        return false;
    };
    ancestors(node)
        .take_while(|ancestor| !is_executable_scope(ancestor.kind()))
        .any(|ancestor| {
            ancestor.kind() == "ERROR" && ancestor.byte_range() == recovered.error_range
        })
}

fn declaration_scope(
    node: Node<'_>,
    root: Node<'_>,
    recovered_types: &[super::syntax::RecoveredType],
) -> Range<usize> {
    if let Some(owner) = ancestors(node).find(|ancestor| is_executable_scope(ancestor.kind()))
        && let Some(body) = executable_body(owner)
    {
        return body;
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

fn range(node: Node<'_>) -> Range<usize> {
    node.start_byte()..node.end_byte()
}

fn is_executable_scope(kind: &str) -> bool {
    matches!(
        kind,
        "function_declaration"
            | "secondary_constructor"
            | "anonymous_function"
            | "lambda_literal"
            | "getter"
            | "setter"
            | "anonymous_initializer"
    )
}

fn executable_body(node: Node<'_>) -> Option<Range<usize>> {
    if node.kind() == "lambda_literal" {
        return Some(range(node));
    }
    all_nodes(node)
        .into_iter()
        .filter(|child| child.start_byte() >= node.start_byte())
        .find(|child| child.kind() == "statements" || child.kind() == "function_body")
        .map(range)
        .or_else(|| Some(range(node)))
}

fn class_body_scope(node: Node<'_>) -> Option<Range<usize>> {
    all_nodes(node)
        .into_iter()
        .find(|child| matches!(child.kind(), "class_body" | "enum_class_body"))
        .map(range)
}

fn visibility(node: Node<'_>, source: &[u8]) -> SemanticVisibility {
    if has_modifier(node, source, "private") {
        SemanticVisibility::Private
    } else if has_modifier(node, source, "protected") {
        SemanticVisibility::Protected
    } else if has_modifier(node, source, "internal") {
        SemanticVisibility::Internal
    } else {
        SemanticVisibility::Public
    }
}

fn modifiers(node: Node<'_>, source: &[u8]) -> DeclarationModifiers {
    let contains = |value: &str| has_modifier(node, source, value);
    DeclarationModifiers {
        is_expect: contains("expect"),
        is_actual: contains("actual"),
        is_abstract: contains("abstract"),
        is_open: contains("open"),
        is_final: contains("final"),
        is_override: contains("override"),
        is_operator: contains("operator"),
        is_infix: contains("infix"),
    }
}

fn has_modifier(node: Node<'_>, source: &[u8], value: &str) -> bool {
    named_children(node)
        .into_iter()
        .find(|child| child.kind() == "modifiers")
        .is_some_and(|modifiers| {
            all_nodes(modifiers)
                .into_iter()
                .filter(|child| child.child_count() == 0)
                .any(|child| text(child, source) == value || child.kind() == value)
        })
}

fn annotation_names(node: Node<'_>, source: &[u8]) -> Vec<String> {
    named_children(node)
        .into_iter()
        .find(|child| child.kind() == "modifiers")
        .into_iter()
        .flat_map(all_nodes)
        .filter(|child| child.kind() == "annotation")
        .flat_map(|annotation| all_nodes(annotation))
        .filter(|child| child.kind() == "user_type")
        .map(|child| normalized_type(text(child, source)))
        .collect()
}

fn has_annotation(node: Node<'_>, source: &[u8], name: &str) -> bool {
    annotation_names(node, source)
        .iter()
        .any(|annotation| annotation.rsplit('.').next() == Some(name))
}

fn direct_type_children(node: Node<'_>) -> Vec<Node<'_>> {
    named_children(node)
        .into_iter()
        .filter(|child| is_type_node(child.kind()))
        .collect()
}

fn is_type_node(kind: &str) -> bool {
    matches!(
        kind,
        "user_type"
            | "receiver_type"
            | "function_type"
            | "nullable_type"
            | "not_nullable_type"
            | "parenthesized_type"
            | "dynamic"
    )
}

fn normalized_type(value: String) -> String {
    value
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>()
        .trim_end_matches('.')
        .to_owned()
}

fn declared_type(node: Node<'_>, source: &[u8]) -> Option<String> {
    if node.kind() == "property_declaration" {
        return all_nodes(node)
            .into_iter()
            .find(|child| child.kind() == "variable_declaration")
            .and_then(|variable| direct_type_children(variable).into_iter().next())
            .map(|ty| normalized_type(text(ty, source)));
    }
    if node.kind() == "class_declaration" || node.kind() == "object_declaration" {
        return declaration_name(node, source);
    }
    None
}

fn declaration_supertypes(node: Node<'_>, source: &[u8]) -> Vec<String> {
    if !matches!(node.kind(), "class_declaration" | "object_declaration") {
        return Vec::new();
    }
    named_children(node)
        .into_iter()
        .filter(|child| child.kind() == "delegation_specifier")
        .filter_map(|delegation| {
            all_nodes(delegation)
                .into_iter()
                .find(|child| child.kind() == "user_type")
        })
        .map(|child| normalized_type(text(child, source)))
        .collect()
}

fn alias_target(node: Node<'_>, source: &[u8]) -> Option<String> {
    direct_type_children(node)
        .into_iter()
        .last()
        .map(|ty| normalized_type(text(ty, source)))
}

fn callable_signature(node: Node<'_>, source: &[u8]) -> Option<CallableSignature> {
    if !matches!(
        node.kind(),
        "function_declaration"
            | "secondary_constructor"
            | "class_declaration"
            | "anonymous_function"
            | "property_declaration"
    ) {
        return None;
    }
    let parameter_container = if node.kind() == "class_declaration" {
        primary_constructor_parameters(node)
    } else {
        all_nodes(node)
            .into_iter()
            .find(|child| child.kind() == "function_value_parameters")
            .map(|container| {
                named_children(container)
                    .into_iter()
                    .filter(|child| {
                        matches!(child.kind(), "parameter" | "function_value_parameter")
                    })
                    .collect::<Vec<_>>()
            })
    };
    let parameters = parameter_container
        .unwrap_or_default()
        .into_iter()
        .filter_map(|parameter| parameter_info(parameter, source))
        .collect();
    let receiver_type = node
        .child_by_field_name("receiver")
        .map(|receiver| normalized_type(text(receiver, source)));
    let return_type = if node.kind() == "property_declaration" {
        declared_type(node, source)
    } else if node.kind() == "function_declaration" || node.kind() == "anonymous_function" {
        let params_end = all_nodes(node)
            .into_iter()
            .find(|child| child.kind() == "function_value_parameters")
            .map_or(node.start_byte(), |params| params.end_byte());
        named_children(node)
            .into_iter()
            .find(|child| {
                child.start_byte() >= params_end
                    && is_type_node(child.kind())
                    && child.kind() != "receiver_type"
            })
            .map(|ty| normalized_type(text(ty, source)))
    } else {
        None
    };
    Some(CallableSignature {
        type_parameters: signature_type_parameters(node, source),
        parameters,
        return_type,
        receiver_type,
    })
}

fn signature_type_parameters(node: Node<'_>, source: &[u8]) -> Vec<CallableTypeParameter> {
    let mut result = Vec::new();
    let class = if node.kind() == "class_declaration" {
        Some(node)
    } else {
        ancestors(node).find(|ancestor| ancestor.kind() == "class_declaration")
    };
    if let Some(class) = class
        && class.id() != node.id()
    {
        append_type_parameters(class, source, &mut result);
    }
    if matches!(
        node.kind(),
        "class_declaration" | "function_declaration" | "anonymous_function"
    ) {
        append_type_parameters(node, source, &mut result);
    }
    result
}

fn append_type_parameters(
    declaration: Node<'_>,
    source: &[u8],
    parameters: &mut Vec<CallableTypeParameter>,
) {
    for parameter in named_children(declaration)
        .into_iter()
        .find(|child| child.kind() == "type_parameters")
        .into_iter()
        .flat_map(named_children)
        .filter(|child| child.kind() == "type_parameter")
    {
        let Some(name) = named_children(parameter)
            .into_iter()
            .find(|child| child.kind() == "type_identifier")
            .map(|name| text(name, source).trim_matches('`').to_owned())
        else {
            continue;
        };
        let mut bounds = direct_type_children(parameter)
            .into_iter()
            .map(|bound| normalized_type(text(bound, source)))
            .filter(|bound| !bound.is_empty())
            .collect::<Vec<_>>();
        normalize_bounds(&mut bounds);
        if let Some(existing) = parameters.iter_mut().find(|item| item.name == name) {
            existing.bounds = bounds;
        } else {
            parameters.push(CallableTypeParameter { name, bounds });
        }
    }

    let constraints = named_children(declaration)
        .into_iter()
        .find(|child| child.kind() == "type_constraints");
    for constraint in constraints
        .into_iter()
        .flat_map(named_children)
        .filter(|child| child.kind() == "type_constraint")
    {
        let Some(name) = named_children(constraint)
            .into_iter()
            .find(|child| child.kind() == "type_identifier")
            .map(|name| text(name, source).trim_matches('`').to_owned())
        else {
            continue;
        };
        let Some(parameter) = parameters.iter_mut().find(|item| item.name == name) else {
            continue;
        };
        parameter.bounds.extend(
            direct_type_children(constraint)
                .into_iter()
                .map(|bound| normalized_type(text(bound, source)))
                .filter(|bound| !bound.is_empty()),
        );
        normalize_bounds(&mut parameter.bounds);
    }
}

fn normalize_bounds(bounds: &mut Vec<String>) {
    let mut unique = HashSet::new();
    bounds.retain(|bound| unique.insert(bound.clone()));
}

fn parameter_info(node: Node<'_>, source: &[u8]) -> Option<CallableParameter> {
    let name = named_children(node)
        .into_iter()
        .find(|child| matches!(child.kind(), "simple_identifier" | "type_identifier"))
        .map(|name| text(name, source).trim_matches('`').to_owned());
    let type_name = direct_type_children(node)
        .into_iter()
        .last()
        .map(|ty| normalized_type(text(ty, source)));
    if name.is_none() && type_name.is_none() {
        return None;
    }
    let node_text = text(node, source);
    let default_gap_has_equals = node.parent().is_some_and(|parent| {
        let next_start = named_children(parent)
            .into_iter()
            .filter(|child| child.start_byte() >= node.end_byte())
            .map(|child| child.start_byte())
            .min()
            .unwrap_or(parent.end_byte());
        source
            .get(node.end_byte()..next_start)
            .is_some_and(|gap| gap.contains(&b'='))
    });
    let parameter_prefix_start = source
        .get(..node.start_byte())
        .and_then(|prefix| prefix.iter().rposition(|byte| matches!(byte, b'(' | b',')))
        .map_or(node.start_byte(), |boundary| boundary + 1);
    let has_vararg_prefix = source
        .get(parameter_prefix_start..node.start_byte())
        .is_some_and(|prefix| String::from_utf8_lossy(prefix).contains("vararg"));
    Some(CallableParameter {
        name,
        type_name,
        has_default: node_text.contains('=') || default_gap_has_equals,
        is_vararg: node_text.contains("vararg")
            || has_vararg_prefix
            || all_nodes(node)
                .into_iter()
                .any(|child| child.kind() == "vararg" || text(child, source) == "vararg"),
    })
}

fn primary_constructor_parameters(node: Node<'_>) -> Option<Vec<Node<'_>>> {
    let constructor = named_children(node)
        .into_iter()
        .find(|child| child.kind() == "primary_constructor")?;
    Some(
        all_nodes(constructor)
            .into_iter()
            .filter(|child| child.kind() == "class_parameter")
            .collect(),
    )
}

fn collect_locals(
    root: Node<'_>,
    source: &[u8],
    nodes: &[Node<'_>],
    declared_names: &HashSet<(usize, usize)>,
    result: &mut Capture,
) {
    for node in nodes.iter().copied() {
        match node.kind() {
            "parameter" | "class_parameter" | "variable_declaration" => {
                let Some(name_node) = named_children(node)
                    .into_iter()
                    .find(|child| child.kind() == "simple_identifier")
                else {
                    continue;
                };
                if declared_names.contains(&(name_node.start_byte(), name_node.end_byte())) {
                    continue;
                }
                let Some(scope) = binding_scope(node, root) else {
                    continue;
                };
                let name = text(name_node, source).trim_matches('`').to_owned();
                let type_name = direct_type_children(node)
                    .into_iter()
                    .last()
                    .map(|ty| normalized_type(text(ty, source)));
                let mut alias = None;
                let mut callable_reference = false;
                if node.kind() == "variable_declaration"
                    && let Some(property) = node
                        .parent()
                        .filter(|parent| parent.kind() == "property_declaration")
                    && let Some(initializer) = property_initializer(property, node)
                {
                    callable_reference = initializer.kind() == "callable_reference";
                    if callable_reference {
                        alias =
                            reference_target(initializer, source).map(|(path, _)| path.join("."));
                    } else if matches!(
                        initializer.kind(),
                        "simple_identifier" | "navigation_expression"
                    ) {
                        alias = simple_path(initializer, source).map(|path| path.join("."));
                    }
                }
                let immutable = if node.kind() == "variable_declaration" {
                    !node
                        .parent()
                        .filter(|parent| parent.kind() == "property_declaration")
                        .and_then(|parent| {
                            named_children(parent)
                                .into_iter()
                                .find(|child| child.kind() == "binding_pattern_kind")
                        })
                        .is_some_and(|kind| text(kind, source) == "var")
                } else {
                    true
                };
                push_binding(
                    result,
                    BindingEvidence {
                        name,
                        name_node,
                        scope,
                        type_name,
                        alias_target: alias,
                        callable_reference,
                        immutable,
                    },
                );
            }
            "context_parameter" => {
                let Some(scope) = binding_scope(node, root) else {
                    continue;
                };
                let name_node = named_children(node)
                    .into_iter()
                    .find(|child| child.kind() == "simple_identifier");
                let type_node = direct_type_children(node).into_iter().last();
                if let Some(name_node) = name_node {
                    let name = text(name_node, source).trim_matches('`').to_owned();
                    let type_name = type_node.map(|ty| normalized_type(text(ty, source)));
                    push_binding(
                        result,
                        BindingEvidence {
                            name,
                            name_node,
                            scope,
                            type_name,
                            alias_target: None,
                            callable_reference: false,
                            immutable: true,
                        },
                    );
                } else if let (Some(ty), Some(owner)) = (type_node, nearest_declaration(node)) {
                    let ty = normalized_type(text(ty, source));
                    if let Some(body) = executable_body(owner) {
                        result.semantic.receivers.push(ReceiverScope {
                            start_byte: body.start,
                            end_byte: body.end,
                            receiver_type: ty.clone(),
                            implicit: true,
                            label: Some(ty),
                            dsl_markers: Vec::new(),
                        });
                    }
                }
            }
            _ => {}
        }
    }
}

fn collect_context_facts(
    _root: Node<'_>,
    _source: &[u8],
    nodes: &[Node<'_>],
    contexts: &[super::syntax::ContextParameters],
    result: &mut Capture,
) {
    for declaration in nodes.iter().copied().filter(|node| {
        matches!(
            node.kind(),
            "function_declaration"
                | "property_declaration"
                | "class_declaration"
                | "object_declaration"
        )
    }) {
        let Some(context) = super::syntax::context_for_declaration(declaration, contexts) else {
            continue;
        };
        let Some(scope) = (if matches!(
            declaration.kind(),
            "class_declaration" | "object_declaration"
        ) {
            class_body_scope(declaration)
        } else {
            executable_body(declaration)
        }) else {
            continue;
        };
        for parameter in &context.parameters {
            let type_name = normalized_type(parameter.type_name.clone());
            let markers = result
                .semantic
                .declarations
                .iter()
                .find(|fact| {
                    fact.kind == DeclarationKind::Class
                        && fact.start_byte < declaration.start_byte()
                        && fact.declared_type.as_deref().is_some_and(|name| {
                            name.rsplit('.').next() == type_name.rsplit('.').next()
                        })
                })
                .map(|fact| fact.dsl_markers.clone())
                .unwrap_or_default();
            if parameter.name.is_none() {
                result.semantic.receivers.push(ReceiverScope {
                    start_byte: scope.start,
                    end_byte: scope.end,
                    receiver_type: type_name.clone(),
                    implicit: true,
                    label: Some(type_name.clone()),
                    dsl_markers: markers,
                });
            }
            if let (Some(name), Some(name_range)) = (&parameter.name, &parameter.name_range) {
                result.locals.push(LocalBinding {
                    name: name.clone(),
                    start_byte: name_range.start,
                    end_byte: name_range.end,
                    scope_start: scope.start,
                    scope_end: scope.end,
                    context_id: 0,
                });
                result.semantic.bindings.push(BindingType {
                    name: name.clone(),
                    name_start_byte: name_range.start,
                    name_end_byte: name_range.end,
                    scope_start: scope.start,
                    scope_end: scope.end,
                    type_name: Some(type_name),
                    alias_target: None,
                    callable_reference: false,
                    immutable: true,
                });
            }
        }
    }
}

struct BindingEvidence<'tree> {
    name: String,
    name_node: Node<'tree>,
    scope: Range<usize>,
    type_name: Option<String>,
    alias_target: Option<String>,
    callable_reference: bool,
    immutable: bool,
}

fn push_binding(result: &mut Capture, evidence: BindingEvidence<'_>) {
    result.locals.push(LocalBinding {
        name: evidence.name.clone(),
        start_byte: evidence.name_node.start_byte(),
        end_byte: evidence.name_node.end_byte(),
        scope_start: evidence.scope.start,
        scope_end: evidence.scope.end,
        context_id: 0,
    });
    result.semantic.bindings.push(BindingType {
        name: evidence.name,
        name_start_byte: evidence.name_node.start_byte(),
        name_end_byte: evidence.name_node.end_byte(),
        scope_start: evidence.scope.start,
        scope_end: evidence.scope.end,
        type_name: evidence.type_name,
        alias_target: evidence.alias_target,
        callable_reference: evidence.callable_reference,
        immutable: evidence.immutable,
    });
}

fn binding_scope(node: Node<'_>, _root: Node<'_>) -> Option<Range<usize>> {
    if matches!(node.kind(), "parameter" | "class_parameter") {
        let owner = nearest_declaration(node)?;
        if owner.kind() == "class_declaration" {
            return class_body_scope(owner).or(Some(range(owner)));
        }
        return executable_body(owner).or(Some(range(owner)));
    }
    if node.kind() == "context_parameter" {
        return nearest_declaration(node).and_then(executable_body);
    }
    let owner = nearest_declaration(node)?;
    if !is_executable_scope(owner.kind()) {
        return None;
    }
    let scope = ancestors(node)
        .take_while(|ancestor| ancestor.start_byte() >= owner.start_byte())
        .find(|ancestor| {
            matches!(
                ancestor.kind(),
                "statements" | "lambda_literal" | "function_body"
            )
        })
        .map(range)
        .or_else(|| executable_body(owner))?;
    // A local is visible only after its full initializer/declaration.
    let start = if node.kind() == "variable_declaration" {
        node.parent()
            .filter(|parent| parent.kind() == "property_declaration")
            .map_or(node.end_byte(), |parent| parent.end_byte())
    } else {
        node.end_byte()
    };
    Some(start..scope.end)
}

fn property_initializer<'tree>(
    property: Node<'tree>,
    declaration: Node<'tree>,
) -> Option<Node<'tree>> {
    named_children(property)
        .into_iter()
        .filter(|child| child.start_byte() >= declaration.end_byte())
        .find(|child| {
            !matches!(
                child.kind(),
                "type_constraints" | "getter" | "setter" | "receiver_type"
            )
        })
}

fn declaration_execution(
    node: Node<'_>,
    root: Node<'_>,
    source: &[u8],
    contexts: &[super::syntax::ContextParameters],
) -> Option<DeclarationExecutionFacts> {
    let mut execution = DeclarationExecutionFacts::default();
    match node.kind() {
        "property_declaration" => {
            let accessors = property_accessors(node);
            execution.getter = accessors
                .iter()
                .copied()
                .find(|child| child.kind() == "getter")
                .and_then(|child| {
                    super::scopes::metric_scope_range_for(root, child, source, contexts)
                })
                .map(source_range);
            execution.setter = accessors
                .iter()
                .copied()
                .find(|child| child.kind() == "setter")
                .and_then(|child| {
                    super::scopes::metric_scope_range_for(root, child, source, contexts)
                })
                .map(source_range);
            if let Some(range) = super::scopes::property_initializer_range(node, contexts) {
                execution.initializers.push(source_range(range));
            }
            let is_immutable = named_children(node)
                .into_iter()
                .find(|child| child.kind() == "binding_pattern_kind")
                .is_some_and(|kind| text(kind, source) == "val");
            let has_delegate = named_children(node)
                .into_iter()
                .any(|child| child.kind() == "property_delegate");
            if is_immutable && !has_delegate && execution.getter.is_none() {
                let declaration = named_children(node)
                    .into_iter()
                    .find(|child| child.kind() == "variable_declaration");
                execution.callable_value = declaration
                    .and_then(|declaration| property_initializer(node, declaration))
                    .and_then(|initializer| match initializer.kind() {
                        "lambda_literal" | "anonymous_function" => {
                            super::scopes::metric_scope_range_for(
                                root,
                                initializer,
                                source,
                                contexts,
                            )
                        }
                        "annotated_lambda" => all_nodes(initializer)
                            .into_iter()
                            .find(|child| child.kind() == "lambda_literal")
                            .and_then(|child| {
                                super::scopes::metric_scope_range_for(root, child, source, contexts)
                            }),
                        _ => None,
                    })
                    .map(source_range);
            }
        }
        "class_declaration" | "object_declaration" | "companion_object" => {
            if let Some(primary) = named_children(node)
                .into_iter()
                .find(|child| child.kind() == "primary_constructor")
                && let Some(range) =
                    super::scopes::metric_scope_range_for(root, primary, source, contexts)
            {
                execution.constructors.push(SourceRange {
                    start_byte: range.start,
                    end_byte: range.end,
                });
            }
            if let Some(body_node) = named_children(node)
                .into_iter()
                .find(|child| matches!(child.kind(), "class_body" | "enum_class_body"))
            {
                for member in named_children(body_node) {
                    match member.kind() {
                        "secondary_constructor" => {
                            if let Some(range) = super::scopes::metric_scope_range_for(
                                root, member, source, contexts,
                            ) {
                                execution.constructors.push(SourceRange {
                                    start_byte: range.start,
                                    end_byte: range.end,
                                });
                            }
                        }
                        "property_declaration" => {
                            if let Some(range) =
                                super::scopes::property_initializer_range(member, contexts)
                            {
                                execution.initializers.push(SourceRange {
                                    start_byte: range.start,
                                    end_byte: range.end,
                                });
                            }
                        }
                        "anonymous_initializer" => {
                            if let Some(range) = super::scopes::metric_scope_range_for(
                                root, member, source, contexts,
                            ) {
                                execution.initializers.push(SourceRange {
                                    start_byte: range.start,
                                    end_byte: range.end,
                                });
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        "secondary_constructor" => {
            if let Some(range) = super::scopes::metric_scope_range_for(root, node, source, contexts)
            {
                execution.constructors.push(SourceRange {
                    start_byte: range.start,
                    end_byte: range.end,
                });
            }
        }
        _ => {}
    }
    (!execution.constructors.is_empty()
        || !execution.initializers.is_empty()
        || execution.getter.is_some()
        || execution.setter.is_some()
        || execution.callable_value.is_some())
    .then_some(execution)
}

pub(super) fn recovered_type_execution(
    recovered: &super::syntax::RecoveredType,
    root: Node<'_>,
    source: &[u8],
    contexts: &[super::syntax::ContextParameters],
    recovered_types: &[super::syntax::RecoveredType],
) -> Option<DeclarationExecutionFacts> {
    let mut execution = DeclarationExecutionFacts::default();
    let nodes = all_nodes(root);
    for constructor in nodes.iter().copied().filter(|node| {
        node.kind() == "primary_constructor"
            && recovered.keyword_range.end <= node.start_byte()
            && node.end_byte() <= recovered.body_range.start
            && super::syntax::recovered_type_for(*node, recovered_types)
                .is_some_and(|owner| owner.declaration_range == recovered.declaration_range)
    }) {
        if let Some(range) =
            super::scopes::metric_scope_range_for(root, constructor, source, contexts)
        {
            execution.constructors.push(source_range(range));
        }
    }
    for member in nodes.into_iter().filter(|member| {
        recovered.body_range.start <= member.start_byte()
            && member.end_byte() <= recovered.body_range.end
            && super::syntax::recovered_type_for(*member, recovered_types)
                .is_some_and(|owner| owner.declaration_range == recovered.declaration_range)
            && !ancestors(*member).any(|ancestor| {
                ancestor.start_byte() > recovered.declaration_range.start
                    && matches!(
                        ancestor.kind(),
                        "class_declaration" | "object_declaration" | "companion_object"
                    )
            })
    }) {
        match member.kind() {
            "secondary_constructor" => {
                if let Some(range) =
                    super::scopes::metric_scope_range_for(root, member, source, contexts)
                {
                    execution.constructors.push(source_range(range));
                }
            }
            "property_declaration" => {
                if let Some(range) = super::scopes::property_initializer_range(member, contexts) {
                    execution.initializers.push(source_range(range));
                }
            }
            "anonymous_initializer" | "enum_entry" => {
                if let Some(range) =
                    super::scopes::metric_scope_range_for(root, member, source, contexts)
                {
                    execution.initializers.push(source_range(range));
                }
            }
            _ => {}
        }
    }
    (!execution.constructors.is_empty() || !execution.initializers.is_empty()).then_some(execution)
}

fn property_accessors(property: Node<'_>) -> Vec<Node<'_>> {
    let mut accessors = named_children(property)
        .into_iter()
        .filter(|child| matches!(child.kind(), "getter" | "setter"))
        .collect::<Vec<_>>();
    let Some(parent) = property.parent() else {
        return accessors;
    };
    let siblings = named_children(parent);
    let Some(position) = siblings
        .iter()
        .position(|sibling| sibling.id() == property.id())
    else {
        return accessors;
    };
    for sibling in siblings.iter().skip(position + 1).copied() {
        if matches!(sibling.kind(), "line_comment" | "multiline_comment") {
            continue;
        }
        if matches!(sibling.kind(), "getter" | "setter") {
            if !accessors
                .iter()
                .any(|accessor| accessor.id() == sibling.id())
            {
                accessors.push(sibling);
            }
            continue;
        }
        break;
    }
    accessors
}

fn source_range(range: Range<usize>) -> SourceRange {
    SourceRange {
        start_byte: range.start,
        end_byte: range.end,
    }
}

fn nearest_declaration(node: Node<'_>) -> Option<Node<'_>> {
    ancestors(node).find(|ancestor| {
        matches!(
            ancestor.kind(),
            "function_declaration"
                | "secondary_constructor"
                | "anonymous_function"
                | "lambda_literal"
                | "class_declaration"
                | "object_declaration"
                | "companion_object"
                | "getter"
                | "setter"
                | "anonymous_initializer"
        )
    })
}

fn collect_receivers(
    _root: Node<'_>,
    _source: &[u8],
    _nodes: &[Node<'_>],
    _dsl_markers: &HashSet<String>,
    _semantic: &mut SemanticFacts,
) {
    // Receiver scopes are emitted with their declaration so their byte ranges
    // and class ownership stay aligned with the same syntax node.
}

fn collect_calls(nodes: &[Node<'_>], source: &[u8]) -> Vec<CallFacts> {
    nodes
        .iter()
        .copied()
        .filter(|node| matches!(node.kind(), "call_expression" | "constructor_invocation"))
        .map(|call| {
            let children = named_children(call);
            let suffix = children
                .iter()
                .copied()
                .find(|child| child.kind() == "call_suffix");
            let argument_containers = children
                .iter()
                .copied()
                .chain(suffix.into_iter().flat_map(named_children))
                .filter(|child| child.kind() == "value_arguments")
                .collect::<Vec<_>>();
            let mut arguments = argument_containers
                .into_iter()
                .flat_map(named_children)
                .filter(|argument| argument.kind() == "value_argument")
                .filter_map(|argument| call_argument(argument, source, false))
                .collect::<Vec<_>>();
            if let Some(suffix) = suffix {
                arguments.extend(
                    named_children(suffix)
                        .into_iter()
                        .filter(|child| child.kind() == "annotated_lambda")
                        .filter_map(|annotated| {
                            all_nodes(annotated)
                                .into_iter()
                                .find(|child| child.kind() == "lambda_literal")
                        })
                        .filter_map(|lambda| call_argument(lambda, source, true)),
                );
            }
            let target = children
                .iter()
                .copied()
                .find(|child| !matches!(child.kind(), "call_suffix" | "value_arguments"))
                .and_then(|node| reference_target(node, source))
                .map(|(_, target)| SourceRange {
                    start_byte: target.start,
                    end_byte: target.end,
                });
            CallFacts {
                start_byte: call.start_byte(),
                end_byte: call.end_byte(),
                target,
                receiver_type: None,
                arguments,
                ..CallFacts::default()
            }
        })
        .collect()
}

fn call_argument(
    node: Node<'_>,
    source: &[u8],
    is_trailing_lambda: bool,
) -> Option<CallArgumentFact> {
    let (lambda, name, mut type_name, expression, start_byte, end_byte) =
        if node.kind() == "lambda_literal" {
            (
                Some(node),
                None,
                None,
                node,
                node.start_byte(),
                node.end_byte(),
            )
        } else if node.kind() == "value_argument" {
            let children = named_children(node);
            let argument_name = children
                .iter()
                .copied()
                .find(|child| is_argument_label(*child, node, source))
                .map(|name| text(name, source).trim_matches('`').to_owned());
            let expression = children.iter().copied().rfind(|child| {
                child.kind() != "annotation" && !is_argument_label(*child, node, source)
            });
            let lambda = expression.and_then(|expression| match expression.kind() {
                "lambda_literal" => Some(expression),
                "annotated_lambda" => all_nodes(expression)
                    .into_iter()
                    .find(|child| child.kind() == "lambda_literal"),
                _ => None,
            });
            let expression = expression?;
            let type_name = expression_type(expression, source);
            (
                lambda,
                argument_name,
                type_name,
                expression,
                lambda.map_or(expression.start_byte(), |lambda| lambda.start_byte()),
                lambda.map_or(expression.end_byte(), |lambda| lambda.end_byte()),
            )
        } else {
            return None;
        };
    if type_name.is_none() {
        type_name = expression_type(expression, source);
    }
    let explicit_lambda_parameters = lambda
        .and_then(|lambda| {
            named_children(lambda)
                .into_iter()
                .find(|child| child.kind() == "lambda_parameters")
        })
        .into_iter()
        .flat_map(named_children)
        .filter(|parameter| parameter.kind() == "variable_declaration")
        .map(|parameter| {
            direct_type_children(parameter)
                .into_iter()
                .last()
                .map(|ty| normalized_type(text(ty, source)))
        })
        .collect();
    let lambda_parameter_names = lambda
        .and_then(|lambda| {
            named_children(lambda)
                .into_iter()
                .find(|child| child.kind() == "lambda_parameters")
        })
        .into_iter()
        .flat_map(named_children)
        .map(|parameter| {
            named_children(parameter)
                .into_iter()
                .find(|child| child.kind() == "simple_identifier")
                .map(|name| text(name, source).trim_matches('`').to_owned())
                .unwrap_or_default()
        })
        .collect();
    let lambda_has_explicit_parameters = lambda.is_some_and(|lambda| {
        named_children(lambda)
            .into_iter()
            .any(|child| child.kind() == "lambda_parameters")
    });
    Some(CallArgumentFact {
        start_byte,
        end_byte,
        name,
        type_name,
        is_lambda: lambda.is_some(),
        is_trailing_lambda,
        lambda_label: lambda.and_then(|lambda| explicit_lambda_label(lambda, source)),
        lambda_has_explicit_parameters,
        lambda_parameter_names,
        lambda_parameter_types: explicit_lambda_parameters,
        lambda_receiver_type: None,
    })
}

fn explicit_lambda_label(lambda: Node<'_>, source: &[u8]) -> Option<String> {
    let wrapper = lambda.parent()?;
    matches!(wrapper.kind(), "prefix_expression" | "annotated_lambda")
        .then(|| {
            named_children(wrapper)
                .into_iter()
                .find(|child| child.kind() == "label")
        })
        .flatten()
        .map(|label| text(label, source))
        .map(|label| label.trim_end_matches('@').trim().to_owned())
        .filter(|label| !label.is_empty())
}

fn expression_type(node: Node<'_>, source: &[u8]) -> Option<String> {
    match node.kind() {
        "integer_literal" | "hex_literal" | "bin_literal" | "long_literal" | "unsigned_literal" => {
            integer_literal_type(&text(node, source)).map(str::to_owned)
        }
        "real_literal" | "float_literal" => Some(real_literal_type(&text(node, source)).to_owned()),
        "character_literal" => Some("Char".to_owned()),
        "string_literal" | "multi_line_string_literal" => Some("String".to_owned()),
        "boolean_literal" => Some("Boolean".to_owned()),
        _ => None,
    }
}

fn real_literal_type(literal: &str) -> &'static str {
    if literal.ends_with(['f', 'F']) {
        "Float"
    } else {
        "Double"
    }
}

fn integer_literal_type(literal: &str) -> Option<&'static str> {
    let literal = literal.replace('_', "");
    let literal = literal.as_str();
    let lowercase = literal.to_ascii_lowercase();
    let (digits, suffix) = if lowercase.ends_with("ul") || lowercase.ends_with("lu") {
        (&lowercase[..lowercase.len() - 2], "ul")
    } else if lowercase.ends_with('u') {
        (&lowercase[..lowercase.len() - 1], "u")
    } else if lowercase.ends_with('l') {
        (&lowercase[..lowercase.len() - 1], "l")
    } else {
        (lowercase.as_str(), "")
    };
    let (radix, digits) = if let Some(digits) = digits.strip_prefix("0x") {
        (16, digits)
    } else if let Some(digits) = digits.strip_prefix("0b") {
        (2, digits)
    } else {
        (10, digits)
    };
    let value = u64::from_str_radix(digits, radix).ok()?;
    match suffix {
        "ul" => Some("ULong"),
        "u" if value <= u32::MAX as u64 => Some("UInt"),
        "u" => Some("ULong"),
        "l" if value <= i64::MAX as u64 => Some("Long"),
        "l" => None,
        "" if value <= i32::MAX as u64 => Some("Int"),
        "" if value <= i64::MAX as u64 => Some("Long"),
        _ => None,
    }
}

fn enrich_call_argument_types(
    calls: &mut [CallFacts],
    bindings: &[BindingType],
    nodes: &[Node<'_>],
    source: &[u8],
) {
    for call in calls {
        for argument in &mut call.arguments {
            if argument.is_lambda || argument.type_name.is_some() {
                continue;
            }
            let Some(node) = nodes.iter().copied().find(|node| {
                node.kind() == "value_argument"
                    && node.start_byte() <= argument.start_byte
                    && argument.end_byte <= node.end_byte()
            }) else {
                continue;
            };
            let Some(expression) = named_children(node).into_iter().rfind(|child| {
                child.kind() != "annotation" && !is_argument_label(*child, node, source)
            }) else {
                continue;
            };
            if expression.kind() != "simple_identifier" {
                continue;
            }
            let name = text(expression, source).trim_matches('`').to_owned();
            let binding = bindings
                .iter()
                .filter(|binding| {
                    binding.name == name
                        && binding.name_start_byte < expression.start_byte()
                        && binding.scope_start <= expression.start_byte()
                        && expression.start_byte() < binding.scope_end
                })
                .max_by_key(|binding| binding.name_start_byte);
            argument.type_name = binding.and_then(|binding| binding.type_name.clone());
        }
    }
}

fn collect_references(
    root: Node<'_>,
    source: &[u8],
    nodes: &[Node<'_>],
    excluded: &[Range<usize>],
    contexts: &[super::syntax::ContextParameters],
) -> Vec<Reference> {
    let mut references = Vec::new();
    let mut claimed = Vec::<Range<usize>>::new();
    let mut ordered_calls = nodes
        .iter()
        .copied()
        .filter(|node| matches!(node.kind(), "call_expression" | "constructor_invocation"))
        .collect::<Vec<_>>();
    ordered_calls.sort_by_key(|call| (call.start_byte(), call.end_byte()));
    for call in ordered_calls {
        let Some(target) = named_children(call)
            .into_iter()
            .find(|child| !matches!(child.kind(), "call_suffix" | "value_arguments"))
        else {
            continue;
        };
        let Some((path, range)) = reference_target(target, source) else {
            continue;
        };
        if is_excluded(&range, excluded) || claimed.iter().any(|item| contains(item, &range)) {
            continue;
        }
        claimed.push(range.clone());
        let kind = if call.kind() == "call_expression" && target.kind() == "navigation_expression" {
            ReferenceKind::Method
        } else {
            ReferenceKind::Call
        };
        references.push(reference(
            path,
            range,
            kind,
            Some(call),
            root,
            contexts,
            source,
        ));
    }

    for node in nodes
        .iter()
        .copied()
        .filter(|node| node.kind() == "user_type")
    {
        if is_declaration_type(node) {
            continue;
        }
        if let Some((path, range)) = reference_target(node, source)
            && !is_excluded(&range, excluded)
            && !claimed.iter().any(|item| contains(item, &range))
        {
            claimed.push(range.clone());
            references.push(reference(
                path,
                range,
                ReferenceKind::Type,
                None,
                root,
                contexts,
                source,
            ));
        }
    }

    for node in nodes
        .iter()
        .copied()
        .filter(|node| node.kind() == "callable_reference")
    {
        if let Some((path, range)) = reference_target(node, source)
            && !is_excluded(&range, excluded)
            && !claimed.iter().any(|item| contains(item, &range))
        {
            claimed.push(range.clone());
            references.push(reference(
                path,
                range,
                ReferenceKind::CallableReference,
                None,
                root,
                contexts,
                source,
            ));
        }
    }

    for node in nodes
        .iter()
        .copied()
        .filter(|node| matches!(node.kind(), "navigation_expression"))
    {
        if is_store_target(node) {
            continue;
        }
        if let Some((path, range)) = reference_target(node, source)
            && !is_excluded(&range, excluded)
            && !claimed.iter().any(|item| contains(item, &range))
        {
            claimed.push(range.clone());
            references.push(reference(
                path,
                range,
                ReferenceKind::Qualified,
                None,
                root,
                contexts,
                source,
            ));
        }
    }

    for node in nodes
        .iter()
        .copied()
        .filter(|node| matches!(node.kind(), "simple_identifier" | "type_identifier"))
    {
        let range = node.byte_range();
        if is_excluded(&range, excluded)
            || claimed.iter().any(|item| contains(item, &range))
            || is_label(node)
            || is_named_argument(node, source)
            || is_declaration_name(node)
            || is_navigation_member(node)
        {
            continue;
        }
        let path = vec![text(node, source).trim_matches('`').to_owned()];
        references.push(reference(
            path,
            range,
            ReferenceKind::Value,
            None,
            root,
            contexts,
            source,
        ));
    }

    references.sort_by_key(|reference| {
        (
            reference.start_byte,
            reference.end_byte,
            reference.kind as u8,
        )
    });
    references.dedup_by(|left, right| {
        left.start_byte == right.start_byte && left.end_byte == right.end_byte
    });
    references
}

fn reference_target(node: Node<'_>, source: &[u8]) -> Option<(Vec<String>, Range<usize>)> {
    match node.kind() {
        "simple_identifier" | "type_identifier" | "user_type" => {
            Some((simple_path(node, source)?, range(node)))
        }
        "navigation_expression" => {
            let suffix = named_children(node)
                .into_iter()
                .rfind(|child| child.kind() == "navigation_suffix")?;
            let name = named_children(suffix)
                .into_iter()
                .find(|child| matches!(child.kind(), "simple_identifier" | "type_identifier"))?;
            let path = simple_path(node, source)
                .unwrap_or_else(|| vec![text(name, source).trim_matches('`').to_owned()]);
            Some((path, range(name)))
        }
        "callable_reference" => {
            let names = all_nodes(node)
                .into_iter()
                .filter(|child| matches!(child.kind(), "simple_identifier" | "type_identifier"))
                .map(|child| text(child, source).trim_matches('`').to_owned())
                .collect::<Vec<_>>();
            (!names.is_empty()).then_some((names, range(node)))
        }
        _ => None,
    }
}

fn simple_path(node: Node<'_>, source: &[u8]) -> Option<Vec<String>> {
    match node.kind() {
        "simple_identifier" | "type_identifier" => {
            Some(vec![text(node, source).trim_matches('`').to_owned()])
        }
        "user_type" => Some(
            named_children(node)
                .into_iter()
                .filter(|child| matches!(child.kind(), "type_identifier" | "simple_identifier"))
                .map(|child| text(child, source).trim_matches('`').to_owned())
                .collect(),
        ),
        "super_expression" => Some(vec!["super".to_owned()]),
        "this_expression" => {
            let expression = text(node, source);
            let label = expression
                .split_once('@')
                .map(|(_, label)| label.trim().trim_matches('`'));
            Some(label.map_or_else(
                || vec!["this".to_owned()],
                |label| vec![format!("this@{label}")],
            ))
        }
        "navigation_expression" => {
            let mut children = named_children(node).into_iter();
            let left = children.next()?;
            let mut path = simple_path(left, source)?;
            for suffix in children.filter(|child| child.kind() == "navigation_suffix") {
                let member = named_children(suffix).into_iter().find(|child| {
                    matches!(child.kind(), "simple_identifier" | "type_identifier")
                })?;
                path.push(text(member, source).trim_matches('`').to_owned());
            }
            Some(path)
        }
        _ => None,
    }
}

fn is_declaration_type(node: Node<'_>) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    matches!(
        parent.kind(),
        "class_declaration" | "object_declaration" | "type_alias"
    ) && declaration_name_node(parent).is_some_and(|name| name.byte_range() == node.byte_range())
}

fn is_declaration_name(node: Node<'_>) -> bool {
    node.parent().is_some_and(|parent| {
        matches!(
            parent.kind(),
            "function_declaration"
                | "variable_declaration"
                | "class_parameter"
                | "type_alias"
                | "class_declaration"
                | "object_declaration"
                | "companion_object"
        ) && declaration_name_node(parent)
            .is_some_and(|name| name.byte_range() == node.byte_range())
    })
}

fn is_named_argument(node: Node<'_>, source: &[u8]) -> bool {
    node.parent().is_some_and(|parent| {
        parent.kind() == "value_argument" && is_argument_label(node, parent, source)
    })
}

fn is_argument_label(node: Node<'_>, parent: Node<'_>, source: &[u8]) -> bool {
    if node.kind() != "simple_identifier" || parent.kind() != "value_argument" {
        return false;
    }
    let Some(next) = named_children(parent)
        .into_iter()
        .find(|child| child.start_byte() > node.end_byte())
    else {
        return false;
    };
    source
        .get(node.end_byte()..next.start_byte())
        .is_some_and(|between| between.contains(&b'='))
}

fn is_navigation_member(node: Node<'_>) -> bool {
    node.parent()
        .is_some_and(|parent| parent.kind() == "navigation_suffix")
}

fn is_label(node: Node<'_>) -> bool {
    node.parent()
        .is_some_and(|parent| parent.kind() == "label" || parent.kind() == "label_reference")
}

fn is_store_target(node: Node<'_>) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    if matches!(parent.kind(), "assignment" | "postfix_expression") {
        return true;
    }
    if parent.kind() == "navigation_expression" {
        return is_store_target(parent);
    }
    false
}

fn is_excluded(range: &Range<usize>, excluded: &[Range<usize>]) -> bool {
    excluded.iter().any(|item| contains(item, range))
}

fn contains(outer: &Range<usize>, inner: &Range<usize>) -> bool {
    outer.start <= inner.start && inner.end <= outer.end
}

fn reference(
    path: Vec<String>,
    source_range: Range<usize>,
    kind: ReferenceKind,
    call: Option<Node<'_>>,
    root: Node<'_>,
    contexts: &[super::syntax::ContextParameters],
    source: &[u8],
) -> Reference {
    let reference_node = root
        .descendant_for_byte_range(source_range.start, source_range.end)
        .unwrap_or(root);
    let is_default_argument = is_default_argument_reference(reference_node, source);
    let owns_body_call = call.is_some() && !is_default_argument;
    let call_owner_node = call
        .filter(|_| owns_body_call)
        .and_then(|call| ancestors(call).find(|ancestor| is_executable_scope(ancestor.kind())));
    let call_owner_range = call
        .filter(|_| owns_body_call)
        .and_then(|call| super::scopes::metric_scope_range_for(root, call, source, contexts));
    let execution_owner = (!is_default_argument)
        .then(|| super::scopes::metric_scope_range_for(root, reference_node, source, contexts))
        .flatten()
        .map(|owner| owner.start);
    let scope = call_owner_node
        .and_then(executable_body)
        .or_else(|| call_owner_range.clone())
        .or_else(|| {
            let scope = call.into_iter().flat_map(ancestors).find(|ancestor| {
                matches!(
                    ancestor.kind(),
                    "statements" | "class_body" | "enum_class_body" | "lambda_literal"
                )
            })?;
            Some(range(scope))
        })
        .unwrap_or_else(|| range(root));
    Reference {
        path,
        module: ModulePath::default(),
        kind,
        start_byte: source_range.start,
        end_byte: source_range.end,
        scope_start: scope.start,
        scope_end: scope.end,
        call_owner: call_owner_range.map(|owner| owner.start),
        execution_owner,
        access: if matches!(kind, ReferenceKind::Value | ReferenceKind::Qualified) {
            reference_access(reference_node, source)
        } else {
            None
        },
        context_id: 0,
    }
}

fn reference_access(node: Node<'_>, source: &[u8]) -> Option<ReferenceAccess> {
    let assignment = ancestors(node).find(|ancestor| ancestor.kind() == "assignment");
    if let Some(assignment) = assignment {
        let children = named_children(assignment);
        let left = children.first().copied()?;
        if contains_node(left, node) && lvalue_terminal_matches(left, node) {
            let operator = (0..assignment.child_count())
                .filter_map(|index| assignment.child(index))
                .map(|child| text(child, source))
                .find(|text| {
                    matches!(
                        text.as_str(),
                        "=" | "+=" | "-=" | "*=" | "/=" | "%=" | "&&=" | "||="
                    )
                });
            return match operator.as_deref() {
                Some("=") => Some(ReferenceAccess::Write),
                Some("+=") | Some("-=") | Some("*=") | Some("/=") | Some("%=") | Some("&&=")
                | Some("||=") => Some(ReferenceAccess::Read),
                _ => None,
            };
        }
    }
    if let Some(operator) = ancestors(node)
        .find(|ancestor| matches!(ancestor.kind(), "prefix_expression" | "postfix_expression"))
        .and_then(|expression| {
            let operand = named_children(expression)
                .into_iter()
                .find(|child| !matches!(child.kind(), "annotation" | "label"))?;
            if !lvalue_terminal_matches(operand, node) {
                return None;
            }
            (0..expression.child_count())
                .filter_map(|index| expression.child(index))
                .map(|child| text(child, source))
                .find(|text| matches!(text.as_str(), "++" | "--"))
        })
        && matches!(operator.as_str(), "++" | "--")
    {
        return Some(ReferenceAccess::ReadWrite);
    }
    Some(ReferenceAccess::Read)
}

fn lvalue_terminal_matches(lvalue: Node<'_>, reference: Node<'_>) -> bool {
    if all_nodes(lvalue)
        .into_iter()
        .any(|node| node.kind() == "indexing_expression" && node.end_byte() == lvalue.end_byte())
    {
        return false;
    }
    all_nodes(lvalue)
        .into_iter()
        .filter(|node| node.kind() == "simple_identifier")
        .max_by_key(|node| node.start_byte())
        .is_some_and(|node| node.byte_range() == reference.byte_range())
}

fn contains_node(outer: Node<'_>, inner: Node<'_>) -> bool {
    outer.start_byte() <= inner.start_byte() && inner.end_byte() <= outer.end_byte()
}

fn is_default_argument_reference(node: Node<'_>, source: &[u8]) -> bool {
    for ancestor in ancestors(node) {
        if is_executable_scope(ancestor.kind()) {
            return false;
        }
        if matches!(
            ancestor.kind(),
            "parameter" | "function_value_parameter" | "class_parameter"
        ) && source
            .get(ancestor.byte_range())
            .is_some_and(|parameter| parameter.contains(&b'='))
        {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::{all_nodes, collect_calls, integer_literal_type, real_literal_type};

    #[test]
    fn numeric_literal_types_respect_suffixes_and_ranges() {
        assert_eq!(integer_literal_type("42"), Some("Int"));
        assert_eq!(integer_literal_type("2_147_483_648"), Some("Long"));
        assert_eq!(integer_literal_type("42L"), Some("Long"));
        assert_eq!(integer_literal_type("42u"), Some("UInt"));
        assert_eq!(integer_literal_type("4_294_967_296u"), Some("ULong"));
        assert_eq!(integer_literal_type("0xFFFF_FFFFuL"), Some("ULong"));
        assert_eq!(integer_literal_type("9_223_372_036_854_775_808L"), None);
        assert_eq!(integer_literal_type("18_446_744_073_709_551_616u"), None);
        assert_eq!(real_literal_type("1.25"), "Double");
        assert_eq!(real_literal_type("1.25F"), "Float");
    }

    #[test]
    fn call_argument_types_come_from_numeric_literal_cst_wrappers() {
        let source = "fun sample() { consume(1L); consume(1u); consume(0xFF); consume(0b1); consume(1f); consume(1.0F); consume(1.0) }";
        let mut parser = tree_sitter::Parser::new();
        let language: tree_sitter::Language = tree_sitter_kotlin::LANGUAGE.into();
        parser.set_language(&language).unwrap();
        let tree = parser.parse(source, None).unwrap();
        let calls = collect_calls(&all_nodes(tree.root_node()), source.as_bytes());
        let argument_types = calls
            .into_iter()
            .flat_map(|call| {
                call.arguments
                    .into_iter()
                    .map(|argument| argument.type_name)
            })
            .collect::<Vec<_>>();

        assert_eq!(
            argument_types,
            [
                Some("Long".to_owned()),
                Some("UInt".to_owned()),
                Some("Int".to_owned()),
                Some("Int".to_owned()),
                Some("Float".to_owned()),
                Some("Float".to_owned()),
                Some("Double".to_owned()),
            ]
        );
    }
}
