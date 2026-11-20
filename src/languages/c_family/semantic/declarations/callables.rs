use super::*;

impl Records<'_> {
    pub(super) fn lambda(&mut self, node: Node<'_>) {
        let source = self.source;
        if let Some(binding) = node
            .parent()
            .filter(|parent| parent.kind() == "init_declarator")
            && binding
                .parent()
                .and_then(|decl| decl.child_by_field_name("type"))
                .is_some_and(|ty| text(ty, source) == "auto")
        {
            self.data
                .lambdas
                .insert(binding.start_byte(), node.start_byte());
        }
        let name = node
            .parent()
            .filter(|parent| parent.kind() == "init_declarator")
            .and_then(|parent| parent.child_by_field_name("declarator"));
        let name = name.map_or_else(
            || format!("anonymous@{}", node.start_position().row + 1),
            |name| text(name, source),
        );
        self.add(
            node,
            name,
            owner_path(node, source),
            DefinitionKind::Function,
            Symbol {
                body: true,
                internal: true,
                parameters: Some(
                    node.child_by_field_name("declarator")
                        .and_then(|decl| decl.child_by_field_name("parameters"))
                        .into_iter()
                        .flat_map(named_children)
                        .filter(|param| param.kind().contains("parameter_declaration"))
                        .map(|param| declarator::parameter_type(param, source))
                        .collect(),
                ),
                ..Default::default()
            },
        );
    }
    pub(super) fn function(
        &mut self,
        node: Node<'_>,
        range: Node<'_>,
        declarator: Node<'_>,
        mut callable: declarator::Callable,
    ) {
        let source = self.source;
        let kind = node.kind();
        let Some(name) = callable.path.pop() else {
            return;
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
                    p.kind() == "namespace_definition" && p.child_by_field_name("name").is_none()
                }),
            c_linkage,
            dynamic: modifiers("virtual")
                || named_children(declarator).any(|child| child.kind() == "virtual_specifier"),
            member,
            exported: exported(node),
            result: node.child_by_field_name("type").map(|t| text(t, source)),
        };
        self.add(
            range,
            name,
            callable.path,
            if member {
                DefinitionKind::Method
            } else {
                DefinitionKind::Function
            },
            symbol,
        );
    }
}
