use super::*;
use crate::model::ReferenceAccess;

pub(super) fn capture(node: Node<'_>, source: &[u8], data: &mut Data, store: &mut SemanticStore) {
    let Some(parent) = node.parent() else { return };
    let (name, site) = if node.kind() == "field_expression" {
        if parent.kind() == "call_expression"
            && parent.child_by_field_name("function") == Some(node)
        {
            return;
        }
        let Some((field, object)) = node
            .child_by_field_name("field")
            .zip(node.child_by_field_name("argument"))
        else {
            return;
        };
        data.receivers
            .insert(field.start_byte(), text(object, source));
        (text(field, source), field)
    } else {
        let expression = matches!(
            parent.kind(),
            "return_statement"
                | "argument_list"
                | "binary_expression"
                | "assignment_expression"
                | "update_expression"
                | "subscript_expression"
                | "initializer_list"
                | "conditional_expression"
                | "unary_expression"
        ) || parent.child_by_field_name("value") == Some(node)
            && parent.kind() == "init_declarator";
        let name = text(node, source);
        if !expression || calls::binding(store, &name, node.start_byte()).is_some() {
            return;
        }
        (name, node)
    };
    calls::reference(site, path(&name), ReferenceKind::Value, source, store);
    let access = if parent.kind() == "update_expression" {
        ReferenceAccess::ReadWrite
    } else if parent.kind() == "assignment_expression"
        && parent.child_by_field_name("left") == Some(node)
    {
        if parent
            .child_by_field_name("operator")
            .is_some_and(|op| text(op, source) == "=")
        {
            ReferenceAccess::Write
        } else {
            ReferenceAccess::ReadWrite
        }
    } else {
        ReferenceAccess::Read
    };
    store.references.last_mut().unwrap().access = Some(access);
}
