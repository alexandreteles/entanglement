use crate::languages::{CapturedTree, InjectionRequest, syntax::named_children};
use tree_sitter::{InputEdit, Language, Node, Point, Tree};

/// Parse members, statements, or an expression in the same syntactic position as generated Razor code.
/// Remove the temporary wrapper before collecting tokens, functions, and source evidence.
pub(crate) fn capture(
    host: &Tree,
    request: &InjectionRequest,
    included: &[tree_sitter::Range],
    source: &[u8],
    language: &Language,
    tokens: bool,
    parse: impl FnOnce(&[u8]) -> crate::Result<Tree>,
) -> crate::Result<Option<CapturedTree>> {
    let range = &request.range;
    let Some(node) = host
        .root_node()
        .named_descendant_for_byte_range(range.start, range.end)
    else {
        return Ok(None);
    };
    let parent = node.parent().map_or("", |p| p.kind());
    let (prefix, suffix, selector) = match node.kind() {
        "razor_block"
            if node
                .child(1)
                .is_some_and(|token| token.kind() == "at_block") =>
        {
            ("class Template {", "}", "declaration_list")
        }
        "razor_block" => ("class Template { void Render() {", "}}", "block"),
        "variable_declaration" => ("class Template { void Render() {", ";}}", "block"),
        "catch_declaration" => (
            "class Template { void Render() { try {} catch ",
            " {} }}",
            "catch_declaration",
        ),
        kind if kind.ends_with("_statement") => ("class Template { void Render() {", "}}", "block"),
        _ if parent == "razor_case_condition" => (
            "class Template { bool Render(object value) => value is ",
            ";}",
            "is_pattern_expression",
        ),
        _ => (
            "class Template { object Render() => ",
            ";}",
            "arrow_expression_clause",
        ),
    };
    let (Some(first), Some(last)) = (included.first(), included.last()) else {
        return Ok(None);
    };
    let span = first.start_byte..last.end_byte;
    let mut wrapped = Vec::with_capacity(prefix.len() + span.len() + suffix.len());
    wrapped.extend_from_slice(prefix.as_bytes());
    let mut previous = span.start;
    for part in included {
        wrapped.extend(
            source[previous..part.start_byte]
                .iter()
                .map(|&byte| match byte {
                    b'\r' | b'\n' => byte,
                    _ => b' ',
                }),
        );
        wrapped.extend_from_slice(&source[part.start_byte..part.end_byte]);
        previous = part.end_byte;
    }
    let end = wrapped.len();
    let end_point = Point::new(
        last.end_point.row - first.start_point.row,
        if first.start_point.row == last.end_point.row {
            prefix.len() + span.len()
        } else {
            last.end_point.column
        },
    );
    wrapped.extend_from_slice(suffix.as_bytes());
    let mut tree = parse(&wrapped)?;
    let errors = tree.root_node().has_error();
    tree.edit(&InputEdit {
        start_byte: end,
        old_end_byte: wrapped.len(),
        new_end_byte: end,
        start_position: end_point,
        old_end_position: Point::new(end_point.row, end_point.column + suffix.len()),
        new_end_position: end_point,
    });
    let zero = Point::new(0, 0);
    tree.edit(&InputEdit {
        start_byte: 0,
        old_end_byte: prefix.len(),
        new_end_byte: span.start,
        start_position: zero,
        old_end_position: Point::new(0, prefix.len()),
        new_end_position: first.start_point,
    });
    let root = tree.root_node();
    let Some(mut fragment) = descendant(root, selector) else {
        return Ok(None);
    };
    if selector == "arrow_expression_clause"
        && let Some(expression) = fragment.named_child(0)
    {
        fragment = expression;
    }
    let query = super::super::query(language)?;
    let mut captured =
        CapturedTree::from(query.capture_node(fragment, source, tokens, included, |_, _, _| {}));
    super::super::scopes::complete(fragment, source, &mut captured);
    captured.facts = super::super::facts::capture(fragment, source, &captured.functions);
    captured.facts.semantic.structure_errors = errors;
    Ok(Some(captured))
}
fn descendant<'tree>(root: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        if node.kind() == kind {
            return Some(node);
        }
        pending.extend(named_children(node).rev());
    }
    None
}
