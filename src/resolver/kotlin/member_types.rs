use super::*;

impl Index<'_> {
    pub(super) fn is_static_type_qualifier(&self, file: usize, reference: &Reference) -> bool {
        if reference.path.len() < 2 {
            return false;
        }
        let receiver = reference.path[0].as_str();
        if receiver == "this" || receiver.starts_with("this@") || receiver == "super" {
            return false;
        }
        if self
            .graph
            .binding_type(file, receiver, reference.context_id, reference.start_byte)
            .is_some()
            || self.graph.has_local_binding(
                file,
                receiver,
                reference.context_id,
                reference.start_byte,
            )
        {
            return false;
        }
        if self.imported_type(file, reference, receiver).is_some() {
            return true;
        }
        let package = self
            .graph
            .semantic(
                file,
                reference.context_id,
                Some(ResolutionFamily::KotlinModules),
            )
            .and_then(|semantic| semantic.context.as_ref())
            .map_or(&[][..], |semantic| semantic.package.as_slice());
        let candidates = self
            .named(receiver)
            .filter(|candidate| {
                candidate.package == package
                    && candidate.owner.is_empty()
                    && is_type_declaration(candidate)
            })
            .collect::<Vec<_>>();
        candidates.len() == 1
    }
    pub(super) fn type_candidates<'b>(
        &'b self,
        file: usize,
        reference: &Reference,
        type_name: &str,
    ) -> Vec<&'b Candidate> {
        let normalized = normalize_type(type_name);
        let short_name = normalized.rsplit('.').next().unwrap_or(&normalized);
        let package = self.semantic_package(file, reference.context_id);
        let imported = self.imported_candidates(file, reference, short_name, false);
        let mut classes = self
            .named(short_name)
            .filter(|candidate| is_type_declaration(candidate))
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
        classes.dedup_by_key(|candidate| candidate.symbol.clone());
        classes
    }
    pub(super) fn resolve_declared_type(
        &self,
        file: usize,
        context_id: usize,
        type_name: &str,
        package: &[String],
    ) -> Vec<&Candidate> {
        let name = type_name.rsplit('.').next().unwrap_or(type_name);
        if let Some((qualified_package, _)) = type_name.rsplit_once('.') {
            let qualified = qualified_package
                .split('.')
                .map(str::to_owned)
                .chain(std::iter::once(name.to_owned()))
                .collect::<Vec<_>>();
            return self
                .named(name)
                .filter(|candidate| {
                    candidate_type_segments(candidate) == qualified
                        && is_type_declaration(candidate)
                })
                .collect();
        }
        let imported = self
            .graph
            .imports(file)
            .iter()
            .filter(|import| import.context_id == context_id && !import.namespace)
            .filter(|import| {
                import
                    .alias
                    .as_deref()
                    .or_else(|| import.path.last().map(String::as_str))
                    == Some(name)
            })
            .flat_map(|import| {
                let imported_name = import.path.last().map(String::as_str).unwrap_or(name);
                self.named(imported_name).filter(move |candidate| {
                    candidate_type_segments(candidate) == import.path
                        && is_type_declaration(candidate)
                })
            })
            .collect::<Vec<_>>();
        if !imported.is_empty() {
            return imported;
        }
        self.named(name)
            .filter(|candidate| {
                candidate.package == package
                    && candidate.owner.is_empty()
                    && is_type_declaration(candidate)
            })
            .collect()
    }
    pub(super) fn canonical_type_names(
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
        if declarations.len() > 1 {
            return BTreeSet::new();
        }
        if let Some(candidate) = declarations.first() {
            if let Some(alias) = candidate
                .declaration
                .as_ref()
                .and_then(|declaration| declaration.alias_target.as_deref())
            {
                return self.canonical_type_names(
                    candidate.file,
                    candidate.context_id,
                    &candidate.package,
                    alias,
                    seen,
                );
            }
            return [candidate_type_name(candidate)].into_iter().collect();
        }
        let builtins = [
            "Any", "Nothing", "Unit", "Boolean", "Byte", "Short", "Int", "Long", "Float", "Double",
            "Char", "String",
        ];
        if builtins.contains(&normalized.as_str()) || normalized.contains('.') {
            [normalized].into_iter().collect()
        } else {
            BTreeSet::new()
        }
    }
}
