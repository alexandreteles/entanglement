mod emit;
mod parse;

use std::ops::Range;

use tree_sitter::Node;

use super::SemanticFacts;
use super::syntax::{is_top_level, nodes, range};

pub(super) fn collect(
    root: Node<'_>,
    source: &[u8],
    facts: &mut SemanticFacts,
) -> Vec<Range<usize>> {
    let mut excluded = Vec::new();
    for statement in nodes(root)
        .into_iter()
        .filter(|node| matches!(node.kind(), "import_statement" | "import_from_statement"))
    {
        excluded.push(range(statement));
        let module_level = is_top_level(statement);
        for name in parse::imports(statement, source) {
            emit::add_import(statement, &name, root, module_level, facts);
            if module_level && !name.wildcard {
                emit::add_export(statement, &name, root, facts);
            }
        }
    }
    excluded
}
