use super::super::lex;
use super::*;

/// Preserve byte offsets and physical newlines in every parser view.
pub(crate) fn preprocess(source: &[u8], view: &FileView) -> projection::Projection {
    let mut result = source.to_vec();
    let mut visible = vec![false; source.len()];
    for region in &view.regions {
        if region.active != Active::No {
            visible[region.range.clone()].fill(true);
        }
    }
    for (byte, keep) in result.iter_mut().zip(visible) {
        if !keep && !matches!(*byte, b'\r' | b'\n') {
            *byte = b' ';
        }
    }
    projection::Projection::new(&result, view.uses.values())
}

/// Expand only object macros that describe declaration syntax. An expansion
/// that introduces computation has no independently written executable range.
pub(super) fn replacement(name: &str, env: &Environment) -> Option<String> {
    let mut current = name;
    let mut visited = BTreeSet::new();
    while visited.insert(current) {
        let Some(Macro::Defined(value)) = env.macros.get(current) else {
            return None;
        };
        let value = value.trim();
        if env.macros.contains_key(value) {
            current = value;
            continue;
        }
        if value.is_empty() {
            return Some(String::new());
        }
        if value.contains("namespace") {
            let words: Vec<_> = lex::tokens(value.as_bytes())
                .filter(|t| t.kind != lex::Kind::Comment)
                .map(|t| &value[t.range])
                .collect();
            if words.last() == Some(&"{")
                && words.iter().all(|word| {
                    matches!(*word, "{" | "::")
                        || word.chars().all(|c| c.is_alphanumeric() || c == '_')
                })
            {
                return Some(value.into());
            }
        }
        if value.chars().all(|ch| ch == '}' || ch.is_whitespace()) {
            return Some(value.into());
        }
        if value.starts_with("__attribute__") || value.starts_with("__declspec") {
            return Some(String::new());
        }
        if value.split_whitespace().all(|word| {
            matches!(
                word,
                "static"
                    | "extern"
                    | "inline"
                    | "constexpr"
                    | "consteval"
                    | "constinit"
                    | "const"
                    | "volatile"
                    | "noexcept"
                    | "typename"
                    | "class"
                    | "struct"
                    | "virtual"
                    | "explicit"
                    | "friend"
                    | "unsigned"
                    | "signed"
                    | "int"
                    | "long"
                    | "short"
                    | "void"
                    | "char"
            )
        }) {
            return Some(value.into());
        }
        return None;
    }
    None
}
