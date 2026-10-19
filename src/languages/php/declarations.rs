//! Emit PHP declarations and their executable source ranges.

use tree_sitter::Node;

use crate::graph::{
    CallableSignature, DeclarationExecutionFacts, DeclarationFacts, DeclarationKind,
    DeclarationModifiers, PhpDeclaration, PhpSymbolKind, SemanticVisibility,
};
use crate::model::{Definition, DefinitionKind, ModulePath};

use super::Capture;
use super::syntax::*;
use crate::graph::PhpSymbolKind::*;

impl Capture<'_> {
    pub(super) fn declaration(&mut self, node: Node<'_>) {
        match node.kind() {
            "class_declaration"
            | "interface_declaration"
            | "trait_declaration"
            | "enum_declaration"
            | "anonymous_class" => self.class(node),
            "function_definition"
            | "method_declaration"
            | "anonymous_function"
            | "arrow_function"
            | "property_hook" => self.callable(node),
            "property_element" | "property_promotion_parameter" => self.property(node),
            "const_element" | "enum_case" => self.constant(node),
            _ => {}
        }
    }

    fn class(&mut self, node: Node<'_>) {
        let source = self.source;
        let name = declaration_name(node, source);
        let declaration = self.emit(node, &name, Class, None);
        declaration.modifiers.is_final |= node.kind() == "enum_declaration";
        declaration.supertypes = children(node)
            .filter(|child| matches!(child.kind(), "base_clause" | "class_interface_clause"))
            .flat_map(children)
            .filter(|child| is_name(*child))
            .map(|child| text(child, source).to_owned())
            .collect();
        if node.kind() == "anonymous_class" {
            declaration.php.as_mut().expect("PHP metadata").conditional = false;
        }
    }

    fn callable(&mut self, node: Node<'_>) {
        let source = self.source;
        let anonymous = matches!(
            node.kind(),
            "anonymous_function" | "arrow_function" | "property_hook"
        );
        let name = declaration_name(node, source);
        let method = node.kind() == "method_declaration";
        let owner = method
            .then(|| class_name(node, source, &self.php.namespaces))
            .flatten();
        let declaration = self.emit(node, &name, if method { Method } else { Function }, owner);
        declaration.signature = Some(CallableSignature {
            return_type: node
                .child_by_field_name("return_type")
                .map(|kind| text(kind, source).to_owned()),
            ..CallableSignature::default()
        });
        if anonymous {
            declaration.php.as_mut().expect("PHP metadata").conditional = false;
        }
    }

    fn property(&mut self, node: Node<'_>) {
        let source = self.source;
        let Some(name) = node.child_by_field_name("name") else {
            return;
        };
        let Some(owner) = class_name(node, source, &self.php.namespaces) else {
            return;
        };
        let promoted = node.kind() == "property_promotion_parameter";
        let item = if promoted {
            node
        } else {
            node.parent().unwrap_or(node)
        };
        let declaration = self.emit(
            node,
            text(name, source).trim_start_matches(['$', '&']),
            Property,
            Some(owner),
        );
        declaration.visibility = visibility(item, source);
        declaration.declared_type = item
            .child_by_field_name("type")
            .map(|kind| text(kind, source).to_owned());
        declaration.modifiers.is_final = modifier(item, "final_modifier");
        let php_declaration = declaration.php.as_mut().expect("PHP metadata");
        php_declaration.is_static = modifier(item, "static_modifier");
        php_declaration.setter_visibility = children(item)
            .find(|child| {
                child.kind() == "visibility_modifier" && text(*child, source).contains("set")
            })
            .map(|modifier| {
                match text(modifier, source)
                    .split('(')
                    .next()
                    .unwrap_or_default()
                    .trim()
                {
                    "private" => SemanticVisibility::Private,
                    "protected" => SemanticVisibility::Protected,
                    _ => SemanticVisibility::Public,
                }
            })
            .or_else(|| {
                modifier(item, "readonly_modifier").then_some(SemanticVisibility::Protected)
            });
        declaration.modifiers.is_final |=
            php_declaration.setter_visibility == Some(SemanticVisibility::Private);
        if promoted {
            php_declaration.conditional = ancestors(node)
                .find(|node| is_class(*node))
                .is_some_and(conditional);
        }
        let hooks = children(item)
            .filter(|child| child.kind() == "property_hook_list")
            .flat_map(children)
            .filter(|hook| hook.child_by_field_name("body").is_some())
            .collect::<Vec<_>>();
        let hook = |name: &str| {
            hooks
                .iter()
                .find(|hook| {
                    children(**hook)
                        .any(|child| child.kind() == "name" && text(child, source) == name)
                })
                .copied()
                .map(range)
        };
        declaration.execution = Some(DeclarationExecutionFacts {
            getter: hook("get"),
            setter: hook("set"),
            ..DeclarationExecutionFacts::default()
        });
    }

    fn constant(&mut self, node: Node<'_>) {
        let source = self.source;
        let Some(name) = node
            .child_by_field_name("name")
            .or_else(|| children(node).find(|child| child.kind() == "name"))
        else {
            return;
        };
        let owner = class_name(node, source, &self.php.namespaces);
        let declaration = self.emit(node, text(name, source), Constant, owner);
        if let Some(parent) = node.parent() {
            declaration.visibility = visibility(parent, source);
        }
    }

    fn emit(
        &mut self,
        node: Node<'_>,
        name: &str,
        php_kind: PhpSymbolKind,
        owner: Option<String>,
    ) -> &mut DeclarationFacts {
        let source = self.source;
        let php = &mut *self.php;
        let captured = &mut *self.captured;
        let namespace = namespace(&php.namespaces, node);
        let (definition_kind, declaration_kind) = match (php_kind, node.kind()) {
            (Class, "interface_declaration" | "trait_declaration") => {
                (DefinitionKind::Trait, DeclarationKind::Class)
            }
            (Class, "enum_declaration") => (DefinitionKind::Enum, DeclarationKind::Class),
            (Class, _) => (DefinitionKind::Struct, DeclarationKind::Class),
            (Method, _) if name.eq_ignore_ascii_case("__construct") => {
                (DefinitionKind::Method, DeclarationKind::Constructor)
            }
            (Method, _) => (DefinitionKind::Method, DeclarationKind::Function),
            (Function, _) => (DefinitionKind::Function, DeclarationKind::Function),
            (Property, _) => (DefinitionKind::Static, DeclarationKind::Property),
            (Constant, _) => (DefinitionKind::Constant, DeclarationKind::Other),
        };
        captured.definitions.push(Definition {
            name: name.to_owned(),
            kind: definition_kind,
            module: ModulePath(namespace.name.clone()),
            start_byte: node.start_byte(),
            end_byte: node.end_byte(),
            scope_start: namespace.range.start_byte,
            scope_end: namespace.range.end_byte,
            is_public: visibility(node, source) == SemanticVisibility::Public,
            inline_module: false,
            external_module: false,
            context_id: 0,
        });
        let class = ancestors(node).find(|node| is_class(*node));
        captured.semantic.declarations.push(DeclarationFacts {
            php: Some(PhpDeclaration {
                setter_visibility: None,
                kind: php_kind,
                is_static: modifier(node, "static_modifier"),
                conditional: conditional(node),
                is_trait: node.kind() == "trait_declaration",
                is_interface: node.kind() == "interface_declaration",
            }),
            start_byte: node.start_byte(),
            owner: owner.into_iter().collect(),
            visibility: visibility(node, source),
            kind: declaration_kind,
            modifiers: DeclarationModifiers {
                is_abstract: modifier(node, "abstract_modifier"),
                is_final: modifier(node, "final_modifier"),
                ..DeclarationModifiers::default()
            },
            is_interface_member: class.is_some_and(|class| class.kind() == "interface_declaration"),
            ..DeclarationFacts::default()
        });
        captured
            .semantic
            .declarations
            .last_mut()
            .expect("emitted declaration")
    }
}
