use super::*;

use crate::languages::c_family::semantic::Base;

pub(super) type Classes = BTreeMap<(usize, Vec<String>), BTreeSet<Base>>;

pub(super) fn classes(contexts: &[Vec<Context<'_>>]) -> Classes {
    let mut classes: Classes = BTreeMap::new();
    for context in contexts.iter().flatten() {
        if let Some(data) = context.store.semantic.c_family.as_deref() {
            for (name, bases) in &data.classes {
                classes
                    .entry((context.unit, name.clone()))
                    .or_default()
                    .extend(bases.iter().cloned());
            }
        }
    }
    classes
}

impl Index<'_> {
    pub(super) fn base_path(&self, unit: usize, owner: &[String], base: &Base) -> Vec<String> {
        if !base.global {
            for depth in (0..owner.len()).rev() {
                let path = [&owner[..depth], &base.name].concat();
                if self.classes.contains_key(&(unit, path.clone())) {
                    return path;
                }
            }
        }
        base.name.clone()
    }

    pub(super) fn single_bases(&self, unit: usize, owner: Vec<String>) -> Vec<Vec<String>> {
        let mut classes = vec![owner];
        while let Some(current) = classes.last()
            && let Some(bases) = self.classes.get(&(unit, current.clone()))
            && bases.len() == 1
        {
            let next = self.base_path(unit, current, bases.first().unwrap());
            if classes.contains(&next) {
                break;
            }
            classes.push(next);
        }
        classes
    }
    /// An override inherits virtual dispatch even without the override keyword.
    /// An incomplete base type leaves the implementation open.
    pub(super) fn dynamic(&self, entry: &Entry<'_>) -> bool {
        if entry.symbol.dynamic {
            return true;
        }
        let mut visited = BTreeSet::new();
        let mut pending = vec![entry.def.module.0.clone()];
        while let Some(owner) = pending.pop() {
            if !visited.insert(owner.clone()) {
                continue;
            }
            if let Some(bases) = self.classes.get(&(entry.unit, owner.clone())) {
                for base in bases {
                    let base = self.base_path(entry.unit, &owner, base);
                    if !self.classes.contains_key(&(entry.unit, base.clone())) {
                        return true;
                    }
                    if self.names[entry.def.name.as_str()].iter().any(|candidate| {
                        candidate.unit == entry.unit
                            && candidate.def.module.0 == base
                            && candidate.symbol.parameters == entry.symbol.parameters
                            && candidate.symbol.qualifiers == entry.symbol.qualifiers
                            && candidate.symbol.dynamic
                    }) {
                        return true;
                    }
                    pending.push(base);
                }
            }
        }
        false
    }
}
