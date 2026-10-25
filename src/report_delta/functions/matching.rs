//! Match executable scopes independently of positional anonymous display names.

use tree_sitter::InputEdit;

use crate::model::FunctionAnalysis;
use crate::report_delta::edited_position;

mod header;

pub(crate) use header::header_end;

pub(super) fn pair_functions(
    old: &[FunctionAnalysis],
    new: &[FunctionAnalysis],
    old_source: Option<&[u8]>,
    new_source: Option<&[u8]>,
    edits: &[InputEdit],
) -> Vec<Option<usize>> {
    let mut pairs = vec![None; old.len()];
    let mut used = vec![false; new.len()];
    // Reserve every surviving source anchor before considering weaker identities.
    for (index, function) in old.iter().enumerate() {
        let mapped = edited_position(function.start_byte, edits);
        let matches = new
            .iter()
            .enumerate()
            .filter(|(candidate, other)| {
                !used[*candidate]
                    && mapped == Some(other.start_byte)
                    && compatible_names(function, other)
            })
            .map(|(candidate, _)| candidate)
            .collect::<Vec<_>>();
        if let [candidate] = matches.as_slice() {
            pairs[index] = Some(*candidate);
            used[*candidate] = true;
        }
    }
    if let (Some(old_source), Some(new_source)) = (old_source, new_source) {
        // Reorders and whole-line edits can erase edit anchors. Match unchanged
        // source only when its enclosing scope and spelling identify one item.
        pair_unique(old, new, &mut pairs, &mut used, true, |left, right| {
            compatible_names(left, right)
                && owners(old, left, old_source) == owners(new, right, new_source)
                && spelling(old_source, left).is_some()
                && spelling(old_source, left) == spelling(new_source, right)
        });
        // A modified anonymous body can retain a unique call-site/binding anchor.
        pair_unique(old, new, &mut pairs, &mut used, false, |left, right| {
            anonymous(left)
                && anonymous(right)
                && owners(old, left, old_source) == owners(new, right, new_source)
                && anchor(old_source, left).is_some_and(|anchor| !anchor.is_empty())
                && anchor(old_source, left) == anchor(new_source, right)
        });
    }
    // Keep the established unique-name fallback for named declarations. Anonymous
    // line numbers cannot establish identity when a replaced scope disappears.
    pair_unique(old, new, &mut pairs, &mut used, true, |left, right| {
        !anonymous(left) && left.name == right.name
    });
    pairs
}

fn pair_unique(
    old: &[FunctionAnalysis],
    new: &[FunctionAnalysis],
    pairs: &mut [Option<usize>],
    used: &mut [bool],
    full_set: bool,
    equivalent: impl Fn(&FunctionAnalysis, &FunctionAnalysis) -> bool,
) {
    // Exact spelling must identify a unique original scope. Call-site matching
    // may use the remaining set after stronger source matches reserve siblings.
    for (index, function) in old.iter().enumerate() {
        if pairs[index].is_some() {
            continue;
        }
        let candidates = new
            .iter()
            .enumerate()
            .filter(|(candidate, other)| {
                (full_set || !used[*candidate]) && equivalent(function, other)
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if let [candidate] = candidates.as_slice()
            && !used[*candidate]
            && old
                .iter()
                .enumerate()
                .filter(|(index, other)| {
                    (full_set || pairs[*index].is_none()) && equivalent(other, &new[*candidate])
                })
                .count()
                == 1
        {
            pairs[index] = Some(*candidate);
            used[*candidate] = true;
        }
    }
}

fn anonymous(function: &FunctionAnalysis) -> bool {
    function.name.starts_with("anonymous@")
}

fn compatible_names(left: &FunctionAnalysis, right: &FunctionAnalysis) -> bool {
    left.name == right.name || anonymous(left) && anonymous(right)
}

fn spelling<'a>(source: &'a [u8], function: &FunctionAnalysis) -> Option<&'a [u8]> {
    source.get(function.start_byte..function.end_byte)
}

fn owners<'a>(
    functions: &[FunctionAnalysis],
    function: &FunctionAnalysis,
    source: &'a [u8],
) -> Vec<&'a [u8]> {
    let mut enclosing = functions
        .iter()
        .filter(|owner| {
            owner.start_byte < function.start_byte && function.end_byte <= owner.end_byte
        })
        .collect::<Vec<_>>();
    enclosing.sort_by_key(|owner| owner.start_byte);
    enclosing
        .into_iter()
        .filter_map(|owner| {
            let bytes = spelling(source, owner)?;
            // Source headers distinguish overloaded enclosing declarations. For
            // anonymous owners, their call-site anchor supplies the stable context.
            if anonymous(owner) {
                anchor(source, owner)
            } else {
                let end = header_end(bytes);
                Some(bytes[..end].trim_ascii())
            }
        })
        .collect()
}

fn anchor<'a>(source: &'a [u8], function: &FunctionAnalysis) -> Option<&'a [u8]> {
    let prefix = source.get(..function.start_byte)?;
    let start = prefix
        .iter()
        .rposition(|byte| matches!(byte, b';' | b'{' | b'}'))
        .map_or(0, |index| index + 1);
    Some(prefix[start..].trim_ascii())
}
