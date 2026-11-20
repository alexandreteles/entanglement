use super::*;

impl Records<'_> {
    pub(super) fn aggregate(&mut self, node: Node<'_>) {
        let source = self.source;
        let kind = node.kind();
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
                self.data.classes.insert(class, bases);
            }
            let kind = match kind {
                "enum_specifier" => DefinitionKind::Enum,
                "union_specifier" => DefinitionKind::Union,
                _ => DefinitionKind::Struct,
            };
            self.add(
                node,
                text(name, source),
                declarator::owners(node, source),
                kind,
                Symbol {
                    body,
                    ..Default::default()
                },
            );
        }
    }
    pub(super) fn alias(&mut self, node: Node<'_>, name: Node<'_>) {
        self.add(
            node,
            text(name, self.source),
            declarator::owners(node, self.source),
            DefinitionKind::TypeAlias,
            Symbol {
                body: true,
                ..Default::default()
            },
        );
    }
}
