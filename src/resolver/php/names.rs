//! Apply PHP import, namespace fallback, and name-case rules.

use super::*;

pub(super) fn key(kind: PhpSymbolKind, name: &str) -> String {
    match kind {
        PhpSymbolKind::Class | PhpSymbolKind::Function | PhpSymbolKind::Method => {
            name.to_ascii_lowercase()
        }
        PhpSymbolKind::Constant | PhpSymbolKind::Property => name.to_owned(),
    }
}

impl Index<'_> {
    pub(super) fn expanded_names(
        &self,
        file: usize,
        context: usize,
        reference: &PhpReference,
    ) -> Vec<String> {
        let Some(facts) = self.facts(file, context) else {
            return Vec::new();
        };
        let namespace = facts
            .namespaces
            .iter()
            .find(|namespace| namespace.range.start_byte == reference.namespace_start)
            .map_or(String::new(), |namespace| namespace.name.join("\\"));
        let qualify = |name: &str| {
            if namespace.is_empty() {
                name.to_owned()
            } else {
                format!("{namespace}\\{name}")
            }
        };
        let name = &reference.name;
        if let Some(name) = name.strip_prefix('\\') {
            return vec![name.to_owned()];
        }
        if let Some((prefix, suffix)) = name.split_once('\\')
            && prefix.eq_ignore_ascii_case("namespace")
        {
            return vec![qualify(suffix)];
        }
        let kind = reference_kind(reference.kind);
        let (first, suffix) = name
            .split_once('\\')
            .map_or((name.as_str(), None), |(first, rest)| (first, Some(rest)));
        let import_kind = if suffix.is_some() {
            PhpSymbolKind::Class
        } else {
            kind
        };
        let imports = facts
            .imports
            .iter()
            .filter(|import| {
                import.namespace_start == reference.namespace_start
                    && import.kind == import_kind
                    && key(import_kind, &import.alias) == key(import_kind, first)
            })
            .map(|import| match suffix {
                Some(suffix) => format!("{}\\{suffix}", import.path.join("\\")),
                None => import.path.join("\\"),
            })
            .collect::<Vec<_>>();
        if !imports.is_empty() {
            return imports;
        }
        let mut names = vec![qualify(name)];
        if suffix.is_none()
            && !namespace.is_empty()
            && matches!(kind, PhpSymbolKind::Function | PhpSymbolKind::Constant)
        {
            names.push(name.clone());
        }
        names
    }

    pub(super) fn lookup(
        &self,
        file: usize,
        context: usize,
        reference: &PhpReference,
    ) -> Vec<usize> {
        if reference.kind == PhpReferenceKind::Class
            && matches!(
                reference.name.to_ascii_lowercase().as_str(),
                "self" | "parent" | "static"
            )
        {
            let Some(class) = reference.class.as_deref() else {
                return Vec::new();
            };
            let classes = self.classes(file, context, reference.start_byte, &format!("\\{class}"));
            let classes = classes
                .into_iter()
                .flat_map(|class| self.consumers(class, &mut HashSet::new()))
                .collect::<Vec<_>>();
            return if reference.name.eq_ignore_ascii_case("parent") {
                classes
                    .into_iter()
                    .flat_map(|class| self.parents(class))
                    .filter(|parent| !self.candidates[*parent].php.is_interface)
                    .collect()
            } else {
                classes
            };
        }
        let kind = reference_kind(reference.kind);
        let names = self.expanded_names(file, context, reference);
        for name in names {
            let candidates = self
                .names
                .get(&(kind, key(kind, &name)))
                .into_iter()
                .flatten()
                .filter(|index| self.visible(file, context, &self.candidates[**index]))
                .copied()
                .collect::<Vec<_>>();
            if !candidates.is_empty() {
                return candidates;
            }
        }
        Vec::new()
    }

    pub(super) fn classes(
        &self,
        file: usize,
        context: usize,
        position: usize,
        name: &str,
    ) -> Vec<usize> {
        let namespace_start = self
            .facts(file, context)
            .into_iter()
            .flat_map(|facts| &facts.namespaces)
            .filter(|namespace| {
                namespace.range.start_byte <= position && position < namespace.range.end_byte
            })
            .min_by_key(|namespace| namespace.range.end_byte - namespace.range.start_byte)
            .map_or(0, |namespace| namespace.range.start_byte);
        self.lookup(
            file,
            context,
            &PhpReference {
                start_byte: position,
                name: name.to_owned(),
                kind: PhpReferenceKind::Class,
                receiver: None,
                receiver_reference: None,
                namespace_start,
                class: None,
                callable: false,
            },
        )
    }
}

fn reference_kind(kind: PhpReferenceKind) -> PhpSymbolKind {
    match kind {
        PhpReferenceKind::Function => PhpSymbolKind::Function,
        PhpReferenceKind::Constant | PhpReferenceKind::ClassConstant => PhpSymbolKind::Constant,
        PhpReferenceKind::Method | PhpReferenceKind::StaticMethod => PhpSymbolKind::Method,
        PhpReferenceKind::Property | PhpReferenceKind::StaticProperty => PhpSymbolKind::Property,
        _ => PhpSymbolKind::Class,
    }
}
