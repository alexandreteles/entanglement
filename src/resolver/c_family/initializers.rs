use super::*;

impl Index<'_> {
    /// A constructor can initialize its own fields, its direct or virtual bases,
    /// or the same class through a delegating constructor.
    pub(super) fn initializer(&self, context: &Context<'_>, reference: &Reference) -> Outcome {
        let owner = &reference.module.0;
        let name = reference.path.last().unwrap();
        let entries = self
            .names
            .get(name.as_str())
            .into_iter()
            .flatten()
            .filter(|entry| entry.unit == context.unit);
        let global = context
            .store
            .semantic
            .c_family
            .as_ref()
            .is_some_and(|data| data.global_names.contains(&reference.start_byte));
        if reference.path.len() == 1 && !global {
            let fields: Vec<_> = entries
                .clone()
                .filter(|entry| entry.symbol.instance_field && entry.def.module.0 == *owner)
                .map(initializer_target)
                .collect();
            if !fields.is_empty() {
                return combine(fields);
            }
        }
        let mut allowed = BTreeSet::from([owner.clone()]);
        let mut seen = BTreeSet::new();
        let mut pending = vec![owner.clone()];
        while let Some(class) = pending.pop() {
            if !seen.insert(class.clone()) {
                continue;
            }
            for base in self
                .classes
                .get(&(context.unit, class.clone()))
                .into_iter()
                .flatten()
            {
                let path = self.base_path(context.unit, &class, base);
                if class == *owner || base.virtual_base {
                    allowed.insert(path.clone());
                }
                pending.push(path);
            }
        }
        let types = entries.filter(|entry| {
            let qualified = [&entry.def.module.0[..], std::slice::from_ref(name)].concat();
            entry.def.kind == DefinitionKind::Struct
                && entry.symbol.body
                && if global {
                    qualified == reference.path
                } else {
                    qualified.ends_with(&reference.path)
                }
                && allowed.contains(&qualified)
        });
        combine(types.map(initializer_target).collect())
    }
}

fn initializer_target(entry: &Entry<'_>) -> Outcome {
    let target = if entry.position.0 == Active::Yes {
        Resolution::Exact(entry.id.clone())
    } else {
        Resolution::Unresolved
    };
    (target, CallDispatch::Unknown)
}
