use super::*;

impl Inventory<'_> {
    pub(super) fn include(
        &mut self,
        from: &Path,
        name: &str,
        quoted: bool,
        settings: &Settings,
    ) -> Option<PathBuf> {
        let local = from.parent().filter(|_| quoted);
        local
            .into_iter()
            .chain(
                settings
                    .quote
                    .iter()
                    .map(PathBuf::as_path)
                    .filter(|_| quoted),
            )
            .chain(
                settings
                    .include
                    .iter()
                    .chain(&settings.system)
                    .map(PathBuf::as_path),
            )
            .map(|dir| dir.join(name))
            .find_map(|path| {
                let path = crate::input::path_identity(&path).ok()?;
                if !self.sources.contains_key(&path) {
                    let bytes = self.snapshot.read(&path).ok()??;
                    self.sources
                        .insert(path.clone(), Arc::new(Source::new(&bytes)));
                }
                Some(path)
            })
    }
}

pub(super) fn spelling<'a>(value: &'a str, macros: &'a Environment) -> Option<(&'a str, bool)> {
    let value = match macros.macros.get(value) {
        Some(Macro::Defined(value)) => value.trim(),
        _ => value,
    };
    if let Some(value) = value.strip_prefix('"') {
        return Some((value.split_once('"')?.0, true));
    }
    Some((value.strip_prefix('<')?.split_once('>')?.0, false))
}
