use super::*;

pub(super) type Classes = BTreeMap<(usize, Vec<String>), BTreeSet<Vec<String>>>;

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
                    let outer = &owner[..owner.len().saturating_sub(1)];
                    let relative = [outer, base.as_slice()].concat();
                    let base = if self.classes.contains_key(&(entry.unit, relative.clone())) {
                        relative
                    } else {
                        base.clone()
                    };
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
