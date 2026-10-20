//! Borrowed syntax views for static Gradle calls. Build scripts are never executed.

use tree_sitter::{Node, Parser, Tree};

#[derive(Clone, Copy)]
pub(in crate::resolver::kotlin) enum ScriptKind {
    Kotlin,
    Groovy,
}

impl ScriptKind {
    pub(in crate::resolver::kotlin) fn for_path(path: &std::path::Path) -> Self {
        match path.extension().and_then(|extension| extension.to_str()) {
            Some("kts") => Self::Kotlin,
            _ => Self::Groovy,
        }
    }

    pub(super) fn parse(self, source: &str) -> Option<Tree> {
        let language = match self {
            Self::Kotlin => tree_sitter_kotlin::LANGUAGE,
            Self::Groovy => tree_sitter_groovy::LANGUAGE,
        };
        let mut parser = Parser::new();
        parser.set_language(&language.into()).ok()?;
        parser.parse(source, None)
    }
}

pub(super) fn children(node: Node<'_>) -> impl Iterator<Item = Node<'_>> {
    (0..node.named_child_count()).filter_map(move |index| node.named_child(index as u32))
}

pub(super) fn text<'a>(node: Node<'_>, source: &'a str) -> &'a str {
    &source[node.byte_range()]
}

pub(super) fn literal<'a>(node: Node<'_>, source: &'a str) -> Option<&'a str> {
    matches!(node.kind(), "string" | "string_literal").then_some(())?;
    let raw = text(node, source);
    let width = if raw.starts_with("\"\"\"") || raw.starts_with("'''") {
        3
    } else {
        1
    };
    let value = raw.get(width..raw.len().checked_sub(width)?)?;
    (!node.has_error() && !value.contains(['\\', '$'])).then_some(value)
}

pub(super) fn identifier<'a>(node: Node<'_>, source: &'a str) -> Option<&'a str> {
    matches!(node.kind(), "identifier" | "simple_identifier").then(|| text(node, source))
}

/// One direct invocation, with the grammar's argument and body ownership.
pub(super) struct Call<'tree, 'source> {
    pub(super) name: &'source str,
    pub(super) receiver: Option<&'source str>,
    pub(super) args: Vec<Node<'tree>>,
    pub(super) body: Option<Node<'tree>>,
}

impl<'tree, 'source> Call<'tree, 'source> {
    pub(super) fn read(node: Node<'tree>, source: &'source str) -> Option<Self> {
        let (callee, arguments, body) = match node.kind() {
            "call_expression" => {
                let callee = node.named_child(0)?;
                let suffix = children(node).find(|child| child.kind() == "call_suffix")?;
                let arguments = children(suffix).find(|child| child.kind() == "value_arguments");
                let body = children(suffix)
                    .find(|child| child.kind() == "annotated_lambda")
                    .and_then(|lambda| {
                        children(lambda).find(|child| child.kind() == "lambda_literal")
                    })
                    .and_then(|lambda| children(lambda).find(|child| child.kind() == "statements"));
                (callee, arguments, body)
            }
            "function_call" | "juxt_function_call" => {
                let callee = node.child_by_field_name("function")?;
                let arguments = node.child_by_field_name("args");
                let body = arguments
                    .and_then(|args| children(args).find(|child| child.kind() == "closure"));
                (callee, arguments, body)
            }
            _ => return None,
        };
        let (name, receiver) = match callee.kind() {
            "identifier" | "simple_identifier" => (text(callee, source), None),
            "navigation_expression" | "dotted_identifier" => {
                let raw = text(callee, source);
                let (receiver, name) = raw.split_once('.')?;
                (!name.contains('.') && valid_name(receiver) && valid_name(name)).then_some(())?;
                (name, Some(receiver))
            }
            _ => return None,
        };
        let args = arguments
            .into_iter()
            .flat_map(children)
            .filter(|child| !matches!(child.kind(), "closure" | "comment"))
            .filter_map(|child| match child.kind() {
                "value_argument" if child.named_child_count() == 1 => child.named_child(0),
                "value_argument" => Some(child),
                _ => Some(child),
            })
            .collect();
        Some(Self {
            name,
            receiver,
            args,
            body,
        })
    }

    pub(super) fn string(&self, source: &'source str) -> Option<&'source str> {
        match self.args.as_slice() {
            [value] => literal(*value, source),
            _ => None,
        }
    }
}

pub(super) fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

pub(super) fn project_path(path: &str) -> bool {
    path.strip_prefix(':').is_some_and(|path| {
        path.split(':').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
        })
    })
}
