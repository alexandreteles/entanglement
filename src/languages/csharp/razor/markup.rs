use crate::languages::{InjectionRequest, query::ranges, syntax::named_children};
use crate::model::InjectionAnalysis;
use tree_sitter::{Node, Range};

/// Exclude the Razor delimiter and any embedded markup from the C# block.
pub(super) fn blocks(root: Node<'_>, requests: &mut Vec<InjectionRequest>) {
    requests.retain_mut(|request| {
        if request.language != "csharp" {
            return true;
        }
        let Some(block) =
            root.named_descendant_for_byte_range(request.range.start, request.range.end)
        else {
            return false;
        };
        if block.kind() != "razor_block" {
            let kind = block.kind();
            if kind == "element" || kind == "comment" || kind.starts_with("razor_") {
                return false;
            }
            if let Some(parent) = block.parent().filter(|p| p.kind() == "razor_foreach") {
                return parent.child_by_field_name("right") == Some(block)
                    || kind.ends_with("_statement");
            }
            return true;
        }
        let mut cursor = block.walk();
        let Some(open) = block
            .children(&mut cursor)
            .find(|child| child.kind() == "{")
        else {
            return false;
        };
        let Some(close) = block
            .child(block.child_count().saturating_sub(1))
            .filter(|child| child.kind() == "}")
        else {
            return false;
        };
        let body = Range {
            start_byte: open.end_byte(),
            start_point: open.end_position(),
            end_byte: close.start_byte(),
            end_point: close.start_position(),
        };
        let mut pending: Vec<_> = named_children(block).collect();
        let mut excluded = Vec::new();
        while let Some(child) = pending.pop() {
            if child.kind() == "element" || child.kind().starts_with("razor_") {
                excluded.push(child.range());
            } else {
                pending.extend(named_children(child));
            }
        }
        ranges::normalize(&mut excluded);
        request.guest_ranges = ranges::subtract(body, &excluded);
        !request.guest_ranges.is_empty()
    });
}
pub(super) fn has_host_error(root: Node<'_>, guests: &[InjectionAnalysis]) -> bool {
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        if !node.has_error()
            || guests.iter().any(|g| {
                g.analyzed && g.start_byte <= node.start_byte() && node.end_byte() <= g.end_byte
            })
        {
            continue;
        }
        if node.is_error() || node.is_missing() {
            return true;
        }
        pending.extend(node.children(&mut node.walk()));
    }
    false
}
