//! Emit PHP declarations and their executable source ranges.

use tree_sitter::Node;

use crate::graph::{
    CallableSignature, DeclarationExecutionFacts, DeclarationFacts, DeclarationKind,
    DeclarationModifiers, PhpDeclaration, PhpFacts, PhpSymbolKind, SemanticVisibility,
};
use crate::languages::CapturedTree;
use crate::model::{Definition, DefinitionKind, ModulePath};

use super::syntax::*;

pub(super) fn capture(
    nodes: &[Node<'_>],
    source: &[u8],
    php: &mut PhpFacts,
    captured: &mut CapturedTree,
) {
    for node in nodes.iter().copied() {
        match node.kind() {
            "class_declaration"
            | "interface_declaration"
            | "trait_declaration"
            | "enum_declaration"
            | "anonymous_class" => class(node, source, php, captured),
            "function_definition"
            | "method_declaration"
            | "anonymous_function"
            | "arrow_function"
            | "property_hook" => callable(node, source, php, captured),
            "property_element" | "property_promotion_parameter" => {
                property(node, source, php, captured)
            }
            "const_element" | "enum_case" => constant(node, source, php, captured),
            _ => {}
        }
    }
}

fn class(node: Node<'_>, source: &[u8], php: &mut PhpFacts, captured: &mut CapturedTree) {
    let name = node
        .child_by_field_name("name")
        .map(|name| text(name, source).to_owned())
        .unwrap_or_else(|| format!("anonymous-class@{}", node.start_byte()));
    let kind = match node.kind() {
        "interface_declaration" | "trait_declaration" => DefinitionKind::Trait,
        "enum_declaration" => DefinitionKind::Enum,
        _ => DefinitionKind::Struct,
    };
    emit(
        node,
        &name,
        kind,
        PhpSymbolKind::Class,
        None,
        source,
        php,
        captured,
    );
    let declaration = captured
        .semantic
        .declarations
        .last_mut()
        .expect("emitted class");
    declaration.kind = DeclarationKind::Class;
    declaration.modifiers.is_final |= node.kind() == "enum_declaration";
    declaration.supertypes = children(node)
        .into_iter()
        .filter(|child| matches!(child.kind(), "base_clause" | "class_interface_clause"))
        .flat_map(children)
        .filter(|child| is_name(*child))
        .map(|child| text(child, source).to_owned())
        .collect();
    let constructors = node
        .child_by_field_name("body")
        .into_iter()
        .flat_map(children)
        .filter(|child| {
            child.kind() == "method_declaration"
                && child
                    .child_by_field_name("name")
                    .is_some_and(|name| text(name, source).eq_ignore_ascii_case("__construct"))
                && child.child_by_field_name("body").is_some()
        })
        .map(range)
        .collect();
    declaration.execution = Some(DeclarationExecutionFacts {
        constructors,
        ..DeclarationExecutionFacts::default()
    });
    if node.kind() == "anonymous_class" {
        php.declarations
            .last_mut()
            .expect("emitted PHP class")
            .conditional = false;
    }
}

fn callable(node: Node<'_>, source: &[u8], php: &mut PhpFacts, captured: &mut CapturedTree) {
    let anonymous = matches!(
        node.kind(),
        "anonymous_function" | "arrow_function" | "property_hook"
    );
    let name = node
        .child_by_field_name("name")
        .map(|name| text(name, source).to_owned())
        .unwrap_or_else(|| {
            format!(
                "anonymous@{}:{}",
                node.start_position().row + 1,
                node.start_byte()
            )
        });
    let method = node.kind() == "method_declaration";
    let owner = method
        .then(|| class_name(node, source, &php.namespaces))
        .flatten();
    emit(
        node,
        &name,
        if method {
            DefinitionKind::Method
        } else {
            DefinitionKind::Function
        },
        if method {
            PhpSymbolKind::Method
        } else {
            PhpSymbolKind::Function
        },
        owner,
        source,
        php,
        captured,
    );
    let declaration = captured
        .semantic
        .declarations
        .last_mut()
        .expect("emitted callable");
    declaration.kind = if method && name.eq_ignore_ascii_case("__construct") {
        DeclarationKind::Constructor
    } else {
        DeclarationKind::Function
    };
    declaration.signature = Some(CallableSignature {
        return_type: node
            .child_by_field_name("return_type")
            .map(|kind| text(kind, source).to_owned()),
        ..CallableSignature::default()
    });
    if anonymous {
        declaration.execution = Some(DeclarationExecutionFacts {
            callable_value: Some(range(node)),
            ..DeclarationExecutionFacts::default()
        });
        php.declarations
            .last_mut()
            .expect("emitted PHP callable")
            .conditional = false;
    }
}

fn property(node: Node<'_>, source: &[u8], php: &mut PhpFacts, captured: &mut CapturedTree) {
    let Some(name) = node.child_by_field_name("name") else {
        return;
    };
    let Some(owner) = class_name(node, source, &php.namespaces) else {
        return;
    };
    let promoted = node.kind() == "property_promotion_parameter";
    let item = if promoted {
        node
    } else {
        node.parent().unwrap_or(node)
    };
    emit(
        node,
        text(name, source).trim_start_matches(['$', '&']),
        DefinitionKind::Static,
        PhpSymbolKind::Property,
        Some(owner),
        source,
        php,
        captured,
    );
    let declaration = captured
        .semantic
        .declarations
        .last_mut()
        .expect("emitted property");
    declaration.kind = DeclarationKind::Property;
    declaration.visibility = visibility(item, source);
    declaration.declared_type = item
        .child_by_field_name("type")
        .map(|kind| text(kind, source).to_owned());
    declaration.modifiers.is_final = modifier(item, "final_modifier");
    php.declarations
        .last_mut()
        .expect("emitted PHP property")
        .is_static = modifier(item, "static_modifier");
    php.declarations
        .last_mut()
        .expect("emitted PHP property")
        .setter_visibility = children(item)
        .into_iter()
        .find(|child| child.kind() == "visibility_modifier" && text(*child, source).contains("set"))
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
        .or_else(|| modifier(item, "readonly_modifier").then_some(SemanticVisibility::Protected));
    declaration.modifiers.is_final |= php.declarations.last().is_some_and(|declaration| {
        declaration.setter_visibility == Some(SemanticVisibility::Private)
    });
    if promoted {
        php.declarations
            .last_mut()
            .expect("emitted PHP property")
            .conditional = ancestors(node)
            .find(|node| is_class(*node))
            .is_some_and(conditional);
    }
    let hooks = children(item)
        .into_iter()
        .filter(|child| child.kind() == "property_hook_list")
        .flat_map(children)
        .filter(|hook| hook.child_by_field_name("body").is_some())
        .collect::<Vec<_>>();
    let hook = |name: &str| {
        hooks
            .iter()
            .find(|hook| {
                children(**hook)
                    .iter()
                    .any(|child| child.kind() == "name" && text(*child, source) == name)
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

fn constant(node: Node<'_>, source: &[u8], php: &mut PhpFacts, captured: &mut CapturedTree) {
    let Some(name) = node.child_by_field_name("name").or_else(|| {
        children(node)
            .into_iter()
            .find(|child| child.kind() == "name")
    }) else {
        return;
    };
    let owner = class_name(node, source, &php.namespaces);
    emit(
        node,
        text(name, source),
        DefinitionKind::Constant,
        PhpSymbolKind::Constant,
        owner,
        source,
        php,
        captured,
    );
    if let Some(parent) = node.parent() {
        captured
            .semantic
            .declarations
            .last_mut()
            .expect("emitted constant")
            .visibility = visibility(parent, source);
    }
}

fn emit(
    node: Node<'_>,
    name: &str,
    kind: DefinitionKind,
    php_kind: PhpSymbolKind,
    owner: Option<String>,
    source: &[u8],
    php: &mut PhpFacts,
    captured: &mut CapturedTree,
) {
    let namespace = namespace(&php.namespaces, node);
    captured.definitions.push(Definition {
        name: name.to_owned(),
        kind,
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
    php.declarations.push(PhpDeclaration {
        setter_visibility: None,
        start_byte: node.start_byte(),
        namespace: namespace.name.clone(),
        kind: php_kind,
        is_static: modifier(node, "static_modifier"),
        conditional: conditional(node),
        is_trait: node.kind() == "trait_declaration",
        is_interface: node.kind() == "interface_declaration",
    });
    captured.semantic.declarations.push(DeclarationFacts {
        start_byte: node.start_byte(),
        owner: owner.into_iter().collect(),
        visibility: visibility(node, source),
        kind: DeclarationKind::Other,
        declared_type: None,
        supertypes: Vec::new(),
        alias_target: None,
        signature: None,
        modifiers: DeclarationModifiers {
            is_abstract: modifier(node, "abstract_modifier"),
            is_final: modifier(node, "final_modifier"),
            ..DeclarationModifiers::default()
        },
        is_interface_member: class.is_some_and(|class| class.kind() == "interface_declaration"),
        dsl_markers: Vec::new(),
        execution: None,
    });
}
