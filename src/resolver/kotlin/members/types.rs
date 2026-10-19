use super::*;

impl Index<'_> {
    pub(in crate::resolver::kotlin) fn type_candidates<'b>(
        &'b self,
        file: usize,
        reference: &Reference,
        type_name: &str,
    ) -> Vec<&'b Candidate<'b>> {
        let normalized = normalize_type(type_name);
        let short_name = normalized.rsplit('.').next().unwrap_or(&normalized);
        let package = self.semantic_package(file, reference.context_id);
        let imported = self.imported_candidates(file, reference, short_name, false);
        let mut classes = self
            .named_types(short_name)
            .filter(|candidate| self.visible_in_source_set(file, candidate))
            .filter(|candidate| {
                if normalized.contains('.') {
                    candidate_type_name(candidate) == normalized
                } else if !imported.is_empty() {
                    imported.contains(&candidate.candidate_index)
                } else {
                    candidate.package == package && candidate.owner.is_empty()
                }
            })
            .collect::<Vec<_>>();
        classes.dedup_by_key(|candidate| candidate.symbol);
        classes
    }

    pub(in crate::resolver::kotlin) fn resolve_declared_type(
        &self,
        file: usize,
        context_id: usize,
        type_name: &str,
        package: &[String],
    ) -> Vec<&Candidate<'_>> {
        let name = type_name.rsplit('.').next().unwrap_or(type_name);
        if let Some((qualified_package, _)) = type_name.rsplit_once('.') {
            let qualified = qualified_package
                .split('.')
                .map(str::to_owned)
                .chain(std::iter::once(name.to_owned()))
                .collect::<Vec<_>>();
            return self.qualified_types(name, &qualified).collect();
        }
        let imported = self
            .imports(file, context_id)
            .filter(|import| !import.namespace && imported_name(import) == Some(name))
            .flat_map(|import| {
                let imported_name = import.path.last().map(String::as_str).unwrap_or(name);
                self.qualified_types(imported_name, &import.path)
            })
            .collect::<Vec<_>>();
        if !imported.is_empty() {
            return imported;
        }
        self.package_types(name, package).collect()
    }

    pub(in crate::resolver::kotlin) fn canonical_type_names(
        &self,
        file: usize,
        context_id: usize,
        package: &[String],
        type_name: &str,
        seen: &mut BTreeSet<String>,
    ) -> BTreeSet<String> {
        let normalized = normalize_type(type_name);
        if !seen.insert(normalized.clone()) {
            return BTreeSet::new();
        }
        let declarations = self.resolve_declared_type(file, context_id, &normalized, package);
        const BUILTINS: &[&str] = &[
            "Any", "Nothing", "Unit", "Boolean", "Byte", "Short", "Int", "Long", "Float", "Double",
            "Char", "String",
        ];
        match declarations.as_slice() {
            [candidate] => match candidate
                .declaration
                .and_then(|declaration| declaration.alias_target.as_deref())
            {
                Some(alias) => self.canonical_type_names(
                    candidate.file,
                    candidate.context_id,
                    candidate.package,
                    alias,
                    seen,
                ),
                None => [candidate_type_name(candidate)].into_iter().collect(),
            },
            [] if BUILTINS.contains(&normalized.as_str()) || normalized.contains('.') => {
                [normalized].into_iter().collect()
            }
            _ => BTreeSet::new(),
        }
    }

    /// Return direct parent candidates in the declaring type's source context.
    ///
    /// Keep declaration order and duplicate matches. The caller must select
    /// valid parents and decide how to use unresolved or ambiguous evidence.
    pub(in crate::resolver::kotlin) fn direct_supertypes(
        &self,
        class: &Candidate,
    ) -> Vec<&Candidate<'_>> {
        class
            .declaration
            .into_iter()
            .flat_map(|declaration| &declaration.supertypes)
            .flat_map(|supertype| {
                self.resolve_declared_type(class.file, class.context_id, supertype, class.package)
            })
            .collect()
    }
}
