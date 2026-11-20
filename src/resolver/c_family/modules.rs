use super::*;

#[derive(Default)]
pub(super) struct Modules(BTreeMap<String, BTreeSet<String>>);

impl Modules {
    pub fn new(contexts: &[Vec<Context<'_>>]) -> Self {
        let mut map: BTreeMap<_, BTreeSet<_>> = BTreeMap::new();
        for data in contexts
            .iter()
            .flatten()
            .filter_map(|context| context.store.semantic.c_family.as_deref())
        {
            if let Some(module) = &data.module {
                map.entry(module.clone()).or_default().extend(
                    data.imports
                        .iter()
                        .filter(|item| item.3)
                        .map(|item| absolute(Some(module), &item.2)),
                );
            }
        }
        Self(map)
    }
    pub fn visible(&self, data: &Data, target: Option<&str>, at: usize) -> bool {
        let Some(target) = target else { return false };
        if data.module.as_deref() == Some(target) {
            return true;
        }
        let mut seen = BTreeSet::new();
        let mut pending: Vec<_> = data
            .imports
            .iter()
            .filter(|item| item.0 < at)
            .map(|item| absolute(data.module.as_deref(), &item.2))
            .collect();
        while let Some(name) = pending.pop() {
            if name == target {
                return true;
            }
            if seen.insert(name.clone())
                && let Some(imports) = self.0.get(&name)
            {
                pending.extend(imports.iter().cloned());
            }
        }
        false
    }
}
pub(crate) fn absolute(module: Option<&str>, name: &str) -> String {
    if name.starts_with(':') {
        format!(
            "{}{name}",
            module.unwrap_or("").split(':').next().unwrap_or("")
        )
    } else {
        name.into()
    }
}
