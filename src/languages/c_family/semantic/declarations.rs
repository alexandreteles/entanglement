use super::*;

pub(super) fn capture(node: Node<'_>, source: &[u8], data: &mut Data, store: &mut SemanticStore) {
    let kind = node.kind();
    if matches!(
        kind,
        "class_specifier" | "struct_specifier" | "enum_specifier" | "union_specifier"
    ) {
        let body = node.child_by_field_name("body").is_some();
        if !body
            && node.parent().is_some_and(|parent| {
                !matches!(
                    parent.kind(),
                    "translation_unit"
                        | "field_declaration_list"
                        | "compound_statement"
                        | "declaration_list"
                )
            })
        {
            return;
        }
        let name = node.child_by_field_name("name").or_else(|| {
            node.parent()
                .filter(|parent| parent.kind() == "type_definition")
                .and_then(|parent| parent.child_by_field_name("declarator"))
        });
        if let Some(name) = name {
            if body && matches!(kind, "struct_specifier" | "class_specifier") {
                let mut class = declarator::owners(node, source);
                class.push(text(name, source));
                let bases = named_children(node)
                    .filter(|child| child.kind() == "base_class_clause")
                    .flat_map(named_children)
                    .filter(|child| !matches!(child.kind(), "access_specifier" | "comment"))
                    .map(|base| path(&text(base, source)))
                    .collect();
                data.classes.insert(class, bases);
            }
            let kind = match kind {
                "enum_specifier" => DefinitionKind::Enum,
                "union_specifier" => DefinitionKind::Union,
                _ => DefinitionKind::Struct,
            };
            add(
                node,
                text(name, source),
                declarator::owners(node, source),
                kind,
                Symbol {
                    body,
                    ..Default::default()
                },
                source,
                data,
                store,
            );
        }
        return;
    }
    if kind == "lambda_expression" {
        let name = node
            .parent()
            .filter(|parent| parent.kind() == "init_declarator")
            .and_then(|parent| parent.child_by_field_name("declarator"));
        let name = name.map_or_else(
            || format!("anonymous@{}", node.start_position().row + 1),
            |name| text(name, source),
        );
        add(
            node,
            name,
            owner_path(node, source),
            DefinitionKind::Function,
            Symbol {
                body: true,
                internal: true,
                ..Default::default()
            },
            source,
            data,
            store,
        );
        return;
    }
    if kind == "alias_declaration" {
        if let Some(name) = node.child_by_field_name("name") {
            add(
                node,
                text(name, source),
                declarator::owners(node, source),
                DefinitionKind::TypeAlias,
                Symbol {
                    body: true,
                    ..Default::default()
                },
                source,
                data,
                store,
            );
        }
        return;
    }
    let mut cursor = node.walk();
    let declarations: Vec<_> = node
        .children_by_field_name("declarator", &mut cursor)
        .collect();
    for declarator in declarations {
        let range = if node.kind() == "function_definition" || node.named_child_count() <= 2 {
            node
        } else {
            declarator
        };
        if kind == "type_definition" {
            if let Some(name) = declarator::identifier(declarator) {
                add(
                    range,
                    text(name, source),
                    declarator::owners(node, source),
                    DefinitionKind::TypeAlias,
                    Symbol {
                        body: true,
                        ..Default::default()
                    },
                    source,
                    data,
                    store,
                );
            }
        } else if !kind.contains("parameter_declaration")
            && let Some(mut callable) = declarator::callable(declarator, source)
        {
            let Some(name) = callable.path.pop() else {
                continue;
            };
            let modifiers = |word: &str| {
                (0..node.child_count())
                    .filter_map(|i| node.child(i))
                    .any(|child| {
                        child.kind() == word
                            || child.named_child_count() == 0 && text(child, source) == word
                    })
            };
            let member = kind == "field_declaration"
                || ancestors(node).any(|p| p.kind() == "field_declaration_list");
            let c_linkage = ancestors(node).any(|p| {
                p.kind() == "linkage_specification"
                    && p.child_by_field_name("value")
                        .is_some_and(|lang| text(lang, source) == "\"C\"")
            });
            let symbol = Symbol {
                minimum: callable.minimum,
                variadic: callable.variadic,
                parameters: Some(callable.parameters),
                qualifiers: callable.qualifiers,
                body: kind == "function_definition"
                    && node.child_by_field_name("body").is_some_and(|body| {
                        !matches!(
                            body.kind(),
                            "default_method_clause" | "delete_method_clause"
                        )
                    }),
                internal: !member && modifiers("static")
                    || ancestors(node).any(|p| {
                        p.kind() == "namespace_definition"
                            && p.child_by_field_name("name").is_none()
                    }),
                c_linkage,
                dynamic: modifiers("virtual")
                    || named_children(declarator).any(|child| child.kind() == "virtual_specifier"),
                member,
                exported: exported(node),
                result: node.child_by_field_name("type").map(|t| text(t, source)),
            };
            add(
                range,
                name,
                callable.path,
                if member {
                    DefinitionKind::Method
                } else {
                    DefinitionKind::Function
                },
                symbol,
                source,
                data,
                store,
            );
        } else if let Some(name) = declarator::identifier(declarator) {
            let owner = scope(node);
            if kind.contains("parameter_declaration")
                && !matches!(owner.kind(), "function_definition" | "lambda_expression")
            {
                continue;
            }
            let name = text(name, source);
            let type_name = variable_type(node, declarator, source);
            store.locals.push(LocalBinding {
                name,
                start_byte: declarator.start_byte(),
                end_byte: declarator.end_byte(),
                scope_start: owner.start_byte(),
                scope_end: owner.end_byte(),
                type_name,
                callable_reference: declarator.kind() == "function_declarator",
                ..Default::default()
            });
        }
    }
}

fn variable_type(node: Node<'_>, mut declaration: Node<'_>, source: &[u8]) -> Option<String> {
    let ty = node.child_by_field_name("type")?;
    let mut words: Vec<_> = named_children(node)
        .filter(|child| child.kind() == "type_qualifier")
        .map(|child| text(child, source))
        .collect();
    words.push(text(ty, source));
    let mut written = words.join(" ");
    while let Some(child) = declaration.child_by_field_name("declarator") {
        match declaration.kind() {
            "pointer_declarator" => written.push('*'),
            "reference_declarator" => written.push('&'),
            "array_declarator" => written.push('*'),
            _ => {}
        }
        declaration = child;
    }
    Some(written)
}

#[allow(clippy::too_many_arguments)]
fn add(
    node: Node<'_>,
    name: String,
    owners: Vec<String>,
    kind: DefinitionKind,
    mut symbol: Symbol,
    source: &[u8],
    data: &mut Data,
    store: &mut SemanticStore,
) {
    let local = scope(node);
    symbol.exported |= exported(node);
    store.definitions.push(Definition {
        name: name.clone(),
        kind,
        module: ModulePath(owners),
        start_byte: node.start_byte(),
        end_byte: node.end_byte(),
        scope_start: local.start_byte(),
        scope_end: local.end_byte(),
        is_public: !symbol.internal && public_member(node, source),
        inline_module: false,
        external_module: false,
        context_id: 0,
        declaration: None,
    });
    data.symbols.insert((node.start_byte(), name), symbol);
}
