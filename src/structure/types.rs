use serde::{Deserialize, Serialize};

/// A written type expression. Children retain order and repeated occurrences.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypeExpression {
    pub text: String,
    pub form: String,
    pub children: Vec<TypeExpression>,
}

impl TypeExpression {
    /// Retain the grammar structure and every repeated type occurrence.
    pub(crate) fn from_node(node: tree_sitter::Node<'_>, source: &[u8]) -> Self {
        let mut cursor = node.walk();
        Self {
            text: String::from_utf8_lossy(&source[node.start_byte()..node.end_byte()]).into_owned(),
            form: node.kind().to_owned(),
            children: node
                .children(&mut cursor)
                .filter(|child| !child.kind().contains("comment"))
                .map(|child| Self::from_node(child, source))
                .collect(),
        }
    }

    /// Return a comparison key without source spacing or comments.
    pub(crate) fn semantic_key(&self) -> (String, String, Vec<String>) {
        (
            self.form.clone(),
            if self.children.is_empty() {
                self.text.clone()
            } else {
                String::new()
            },
            self.children
                .iter()
                .map(|child| format!("{:?}", child.semantic_key()))
                .collect(),
        )
    }
}

/// A public type with its written and justified alias-expanded form.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypeStructure {
    pub written: TypeExpression,
    pub expanded: Option<TypeExpression>,
    pub origin: String,
}
