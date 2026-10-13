use super::super::*;
use super::header::{recovered_signature, recovered_supertypes};
use super::modifiers::{recovered_annotations, recovered_modifiers, recovered_visibility};

pub(in crate::languages::kotlin::facts) fn collect_recovered_type_definitions(
    context: &DefinitionContext<'_, '_>,
    result: &mut Capture,
    declarations: &mut Vec<DeclarationFacts>,
) {
    let root = context.root;
    let nodes = context.nodes;
    let source = context.source;
    let package = context.package;
    let recovered_types = context.recovered_types;
    let scope_index = context.scope_index;
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
            crate::languages::kotlin::syntax::RecoveredTypeKind::Class
            | crate::languages::kotlin::syntax::RecoveredTypeKind::Interface => {
                DeclarationKind::Class
            }
            crate::languages::kotlin::syntax::RecoveredTypeKind::Object => DeclarationKind::Object,
            crate::languages::kotlin::syntax::RecoveredTypeKind::CompanionObject => {
                DeclarationKind::CompanionObject
            }
        };
        let visibility = recovered_visibility(error, recovered, source);
        let modifier_flags = recovered_modifiers(error, recovered, source);
        let scope = declaration_scope(error, root, recovered_types);
        let definition_kind = match recovered.kind {
            crate::languages::kotlin::syntax::RecoveredTypeKind::Class => DefinitionKind::Struct,
            crate::languages::kotlin::syntax::RecoveredTypeKind::Interface => DefinitionKind::Trait,
            crate::languages::kotlin::syntax::RecoveredTypeKind::Object => DefinitionKind::Struct,
            crate::languages::kotlin::syntax::RecoveredTypeKind::CompanionObject => {
                DefinitionKind::Other
            }
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
            execution: recovered_type_execution(recovered, recovered_types, scope_index),
        });
    }
}

fn recovered_owner_path(
    error: Node<'_>,
    declaration: &crate::languages::kotlin::syntax::RecoveredType,
    source: &[u8],
    recovered_types: &[crate::languages::kotlin::syntax::RecoveredType],
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
