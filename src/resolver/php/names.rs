//! Apply PHP import, namespace fallback, and name-case rules.

use super::*;

pub(super) fn key(kind: PhpSymbolKind, name: &str) -> String {
    match kind {
        PhpSymbolKind::Class | PhpSymbolKind::Function | PhpSymbolKind::Method => {
            name.to_ascii_lowercase()
        }
        _ => name.to_owned(),
    }
}

impl Index<'_> {
    pub(super) fn expanded_names(
        &self,
        site: Site,
        name: &str,
        kind: PhpSymbolKind,
    ) -> Vec<String> {
        let Some(facts) = self.facts(site.file, site.context) else {
            return Vec::new();
        };
        if let Some(name) = name.strip_prefix('\\') {
            return vec![name.to_owned()];
        }
        let region = facts.namespace_at(site.position);
        let namespace = region.map_or(String::new(), |namespace| namespace.name.join("\\"));
        let qualify = |name: &str| match namespace.is_empty() {
            true => name.to_owned(),
            false => format!("{namespace}\\{name}"),
        };
        let (first, suffix) = name
            .split_once('\\')
            .map_or((name, None), |(first, rest)| (first, Some(rest)));
        if first.eq_ignore_ascii_case("namespace")
            && let Some(suffix) = suffix
        {
            return vec![qualify(suffix)];
        }
        let import_kind = match suffix {
            Some(_) => PhpSymbolKind::Class,
            None => kind,
        };
        let namespace_start = region.map_or(0, |namespace| namespace.range.start_byte);
        let imports = facts
            .imports
            .iter()
            .filter(|import| {
                import.namespace_start == namespace_start
                    && import.kind == import_kind
                    && key(import_kind, &import.alias) == key(import_kind, first)
            })
            .map(|import| match suffix {
                Some(suffix) => format!("{}\\{suffix}", import.path.join("\\")),
                None => import.path.join("\\"),
            })
            .collect::<Vec<_>>();
        match imports.is_empty() {
            false => imports,
            true if suffix.is_none()
                && !namespace.is_empty()
                && matches!(kind, PhpSymbolKind::Function | PhpSymbolKind::Constant) =>
            {
                vec![qualify(name), name.to_owned()]
            }
            true => vec![qualify(name)],
        }
    }

    pub(super) fn lookup(
        &self,
        site: Site,
        name: &str,
        kind: PhpSymbolKind,
        lexical: Option<&str>,
    ) -> Vec<usize> {
        match kind == PhpSymbolKind::Class
            && ["self", "parent", "static"]
                .iter()
                .any(|keyword| name.eq_ignore_ascii_case(keyword))
        {
            true => {
                let Some(class) = lexical else {
                    return Vec::new();
                };
                let classes = self
                    .lookup(site, &format!("\\{class}"), kind, None)
                    .into_iter()
                    .flat_map(|class| self.consumers(class, &mut HashSet::new()))
                    .collect::<Vec<_>>();
                match name.eq_ignore_ascii_case("parent") {
                    false => classes,
                    true => classes
                        .into_iter()
                        .flat_map(|class| self.parents(class))
                        .filter(|parent| !self.candidates[*parent].php.is_interface)
                        .collect(),
                }
            }
            false => self
                .expanded_names(site, name, kind)
                .into_iter()
                .find_map(|name| {
                    let candidates = self
                        .symbols
                        .get(&(None, kind, key(kind, &name)))
                        .into_iter()
                        .flatten()
                        .filter(|index| {
                            self.visible(site.file, site.context, &self.candidates[**index])
                        })
                        .copied()
                        .collect::<Vec<_>>();
                    (!candidates.is_empty()).then_some(candidates)
                })
                .unwrap_or_default(),
        }
    }
}

pub(super) fn reference_kind(kind: PhpReferenceKind) -> PhpSymbolKind {
    match kind {
        PhpReferenceKind::Function => PhpSymbolKind::Function,
        PhpReferenceKind::Constant | PhpReferenceKind::ClassConstant => PhpSymbolKind::Constant,
        PhpReferenceKind::Method | PhpReferenceKind::StaticMethod => PhpSymbolKind::Method,
        PhpReferenceKind::Property | PhpReferenceKind::StaticProperty => PhpSymbolKind::Property,
        _ => PhpSymbolKind::Class,
    }
}
