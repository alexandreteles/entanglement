use super::*;

pub(super) fn binary_convention<'tree>(
    kind: &str,
    operator: &str,
    left: Node<'tree>,
    right: Node<'tree>,
) -> Option<(String, Node<'tree>, Option<Node<'tree>>)> {
    let name = match (kind, operator) {
        ("additive_expression", "+") => "plus",
        ("additive_expression", "-") => "minus",
        ("multiplicative_expression", "*") => "times",
        ("multiplicative_expression", "/") => "div",
        ("multiplicative_expression", "%") => "rem",
        ("range_expression", "..") => "rangeTo",
        ("range_expression", "..<") => "rangeUntil",
        ("comparison_expression", "<" | ">" | "<=" | ">=") => "compareTo",
        ("equality_expression", "==" | "!=") => "equals",
        ("check_expression", "in" | "!in") => {
            return Some(("contains".to_owned(), right, Some(left)));
        }
        _ => return None,
    };
    Some((name.to_owned(), left, Some(right)))
}

pub(super) fn infix_parts(node: Node<'_>) -> Option<(Node<'_>, Node<'_>, Node<'_>)> {
    let children = named_children(node);
    let [left, operator, right] = children.as_slice() else {
        return None;
    };
    (operator.kind() == "simple_identifier").then_some((*left, *right, *operator))
}

pub(super) fn binary_operands(node: Node<'_>) -> Option<(Node<'_>, Node<'_>)> {
    let children = named_children(node);
    Some((*children.first()?, *children.last()?))
}

pub(super) fn operator_token(
    node: Node<'_>,
    left: Node<'_>,
    right: Node<'_>,
    source: &[u8],
) -> Option<(String, Range<usize>)> {
    (0..node.child_count())
        .filter_map(|index| node.child(index))
        .filter(|child| {
            !child.is_named()
                && left.end_byte() <= child.start_byte()
                && child.end_byte() <= right.start_byte()
        })
        .find_map(|child| {
            let token = source.get(child.byte_range())?;
            let token = std::str::from_utf8(token).ok()?.trim();
            matches!(
                token,
                "+" | "-"
                    | "*"
                    | "/"
                    | "%"
                    | ".."
                    | "..<"
                    | "<"
                    | ">"
                    | "<="
                    | ">="
                    | "=="
                    | "!="
                    | "in"
                    | "!in"
            )
            .then(|| (token.to_owned(), child.byte_range()))
        })
}

pub(super) fn unary_parts<'tree>(
    node: Node<'tree>,
    source: &[u8],
) -> Option<(Node<'tree>, Range<usize>, String)> {
    let operand = named_children(node)
        .into_iter()
        .find(|child| !matches!(child.kind(), "annotation" | "label"))?;
    let token = (0..node.child_count())
        .filter_map(|index| node.child(index))
        .filter(|child| !child.is_named() && child.end_byte() <= node.end_byte())
        .find(|child| {
            let token = source.get(child.byte_range()).unwrap_or_default();
            matches!(token, b"+" | b"-" | b"!" | b"++" | b"--")
        })?;
    let operator = std::str::from_utf8(source.get(token.byte_range())?)
        .ok()?
        .to_owned();
    Some((operand, token.byte_range(), operator))
}

pub(super) fn unary_convention(operator: &str, kind: &str) -> Option<String> {
    let name = match (kind, operator) {
        ("prefix_expression", "+") => "unaryPlus",
        ("prefix_expression", "-") => "unaryMinus",
        ("prefix_expression", "!") => "not",
        ("prefix_expression" | "postfix_expression", "++") => "inc",
        ("prefix_expression" | "postfix_expression", "--") => "dec",
        _ => return None,
    };
    Some(name.to_owned())
}

pub(super) fn assignment_parts<'tree>(
    node: Node<'tree>,
    source: &[u8],
) -> Option<(Node<'tree>, Node<'tree>, String, Range<usize>)> {
    let children = named_children(node);
    let left = *children.first()?;
    let right = *children.last()?;
    let operator = (0..node.child_count())
        .filter_map(|index| node.child(index))
        .filter(|child| !child.is_named() && left.end_byte() <= child.start_byte())
        .find(|child| child.end_byte() <= right.start_byte())?;
    let value = std::str::from_utf8(source.get(operator.byte_range())?)
        .ok()?
        .to_owned();
    Some((left, right, value, operator.byte_range()))
}

pub(super) fn assignment_convention(operator: &str) -> Option<String> {
    let name = match operator {
        "+=" => "plusAssign",
        "-=" => "minusAssign",
        "*=" => "timesAssign",
        "/=" => "divAssign",
        "%=" => "remAssign",
        _ => return None,
    };
    Some(name.to_owned())
}

pub(super) fn unwrap_indexing(node: Node<'_>) -> Option<Node<'_>> {
    if node.kind() == "indexing_expression" {
        return Some(node);
    }
    if node.kind() == "directly_assignable_expression"
        && all_nodes(node)
            .iter()
            .any(|child| child.kind() == "indexing_suffix")
    {
        return Some(node);
    }
    None
}

pub(super) fn indexing_parts(node: Node<'_>) -> Option<(Node<'_>, Vec<Node<'_>>, Range<usize>)> {
    let children = named_children(node);
    let receiver = *children.first()?;
    let suffix = children
        .iter()
        .copied()
        .find(|child| child.kind() == "indexing_suffix")?;
    let arguments = named_children(suffix)
        .into_iter()
        .filter(|child| is_expression(child.kind()))
        .collect();
    Some((receiver, arguments, suffix.byte_range()))
}

pub(super) fn is_assignment_target(node: Node<'_>) -> bool {
    for parent in ancestors(node) {
        if parent.kind() == "assignment" {
            return parent.named_child(0).is_some_and(|left| {
                left.start_byte() <= node.start_byte() && node.end_byte() <= left.end_byte()
            });
        }
        if matches!(parent.kind(), "statements" | "source_file") {
            return false;
        }
    }
    false
}

pub(super) fn invocation_parts(node: Node<'_>) -> Option<(Node<'_>, Vec<Node<'_>>)> {
    let children = named_children(node);
    let callee = *children.first()?;
    if !matches!(callee.kind(), "simple_identifier" | "this_expression") {
        return None;
    }
    let suffix = children
        .iter()
        .copied()
        .find(|child| child.kind() == "call_suffix")?;
    let arguments = all_nodes(suffix)
        .into_iter()
        .filter(|child| child.kind() == "value_argument")
        .filter_map(|argument| {
            named_children(argument)
                .into_iter()
                .find(|child| is_expression(child.kind()))
        })
        .collect();
    Some((callee, arguments))
}

pub(super) fn is_expression(kind: &str) -> bool {
    matches!(
        kind,
        "simple_identifier"
            | "type_identifier"
            | "this_expression"
            | "super_expression"
            | "call_expression"
            | "navigation_expression"
            | "infix_expression"
            | "additive_expression"
            | "multiplicative_expression"
            | "range_expression"
            | "comparison_expression"
            | "equality_expression"
            | "prefix_expression"
            | "postfix_expression"
            | "indexing_expression"
            | "parenthesized_expression"
            | "literal_constant"
    )
}
