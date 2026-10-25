use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// Return the Python C3 order, with the declaration first.
///
/// Base vectors must use source order. A missing entry is a leaf. The caller
/// must report unresolved bases separately. Return a short error code for a
/// base cycle, a repeated direct base, or an inconsistent precedence order.
pub fn order(unit: &str, parents: &BTreeMap<String, Vec<String>>) -> Result<Vec<String>, String> {
    let mut orders: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut active = BTreeSet::new();
    let mut pending = vec![(unit.to_owned(), false)];
    while let Some((current, expanded)) = pending.pop() {
        if orders.contains_key(&current) {
            continue;
        }
        let bases = parents.get(&current).cloned().unwrap_or_default();
        if expanded {
            let sequences = bases
                .iter()
                .map(|base| VecDeque::from(orders[base].clone()))
                .chain(std::iter::once(bases.iter().cloned().collect()))
                .collect();
            let mut linearized = vec![current.clone()];
            linearized.extend(merge(sequences)?);
            active.remove(&current);
            orders.insert(current, linearized);
            continue;
        }
        if !active.insert(current.clone()) {
            return Err("inheritance_cycle".into());
        }
        if bases.iter().collect::<BTreeSet<_>>().len() != bases.len() {
            return Err("duplicate_base".into());
        }
        pending.push((current, true));
        pending.extend(bases.into_iter().rev().map(|base| (base, false)));
    }
    Ok(orders.remove(unit).unwrap_or_default())
}

fn merge(mut sequences: Vec<VecDeque<String>>) -> Result<Vec<String>, String> {
    let mut result = Vec::new();
    while sequences.iter().any(|sequence| !sequence.is_empty()) {
        let candidate = sequences
            .iter()
            .filter_map(VecDeque::front)
            .find(|candidate| {
                sequences
                    .iter()
                    .all(|sequence| !sequence.iter().skip(1).any(|entry| entry == *candidate))
            })
            .cloned()
            .ok_or_else(|| "inconsistent_mro".to_owned())?;
        for sequence in &mut sequences {
            if sequence.front() == Some(&candidate) {
                sequence.pop_front();
            }
        }
        result.push(candidate);
    }
    Ok(result)
}
