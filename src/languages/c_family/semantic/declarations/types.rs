use super::*;

impl Records<'_> {
    pub(super) fn enumerator(&mut self, node: Node<'_>) {
        let Some(name) = node.child_by_field_name("name") else {
            return;
        };
        let enumeration = ancestors(node).find(|parent| parent.kind() == "enum_specifier");
        let qualifier = enumeration
            .filter(|parent| self.data.cpp && !declarator::scoped_enum(*parent))
            .and_then(|parent| parent.child_by_field_name("name"))
            .map(|name| text(name, self.source));
        let scope = scope(node).kind();
        self.add(
            node,
            text(name, self.source),
            declarator::owners(node, self.source),
            DefinitionKind::Constant,
            Symbol {
                body: true,
                internal: !matches!(scope, "translation_unit" | "field_declaration_list"),
                member: self.data.cpp && scope == "field_declaration_list",
                enum_qualifier: qualifier,
                ..Default::default()
            },
        );
    }

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
                    .flat_map(|clause| bases(clause, source))
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

fn bases(clause: Node<'_>, source: &[u8]) -> Vec<Base> {
    let mut bases = Vec::new();
    let mut virtual_base = false;
    for child in clause.children(&mut clause.walk()) {
        match child.kind() {
            "virtual" => virtual_base = true,
            "," => virtual_base = false,
            "qualified_identifier" | "type_identifier" | "template_type" => {
                let spelling = text(child, source);
                bases.push(Base {
                    name: path(&spelling),
                    global: spelling.starts_with("::"),
                    virtual_base,
                });
            }
            _ => {}
        }
    }
    bases
}
