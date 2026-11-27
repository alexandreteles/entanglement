use super::*;

pub(super) fn compatible(left: &Entry<'_>, right: &Entry<'_>, entries: &[Entry<'_>]) -> bool {
    if left.symbol.parameters != right.symbol.parameters
        || left.symbol.qualifiers != right.symbol.qualifiers
        || left.module != right.module
    {
        return false;
    }
    if left.symbol.result.is_some()
        && right.symbol.result.is_some()
        && left.symbol.result != right.symbol.result
    {
        return false;
    }
    if left.isolated || right.isolated || left.symbol.internal || right.symbol.internal {
        return left.unit == right.unit && left.def.module == right.def.module;
    }
    match (c_linkage(left, entries), c_linkage(right, entries)) {
        (true, true) => true,
        (false, false) => left.def.module == right.def.module,
        _ => false,
    }
}

fn c_linkage(entry: &Entry<'_>, entries: &[Entry<'_>]) -> bool {
    !entry.cpp
        || entry.symbol.c_linkage
        || entries.iter().any(|declaration| {
            declaration.unit == entry.unit
                && declaration.position.0 == Active::Yes
                && declaration.symbol.c_linkage
                && declaration.def.module == entry.def.module
                && declaration.symbol.parameters == entry.symbol.parameters
                && (declaration.position.1, declaration.position.2)
                    <= (entry.position.1, entry.position.2)
        })
}
