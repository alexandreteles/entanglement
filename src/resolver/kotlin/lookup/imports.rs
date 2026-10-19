use super::*;

impl Index<'_> {
    pub(in crate::resolver::kotlin) fn non_extension_imports(
        &self,
        file: usize,
        reference: &Reference,
        name: &str,
    ) -> Vec<usize> {
        self.imported_candidates(file, reference, name, false)
            .into_iter()
            .filter(|index| !is_extension_candidate(&self.candidates[*index]))
            .collect()
    }

    pub(in crate::resolver::kotlin) fn wildcard_candidates(
        &self,
        file: usize,
        reference: &Reference,
        name: &str,
    ) -> Vec<usize> {
        self.imported_candidates(file, reference, name, true)
            .into_iter()
            .filter(|index| {
                !is_extension_candidate(&self.candidates[*index])
                    && !self.is_local_candidate(&self.candidates[*index])
            })
            .collect()
    }

    pub(in crate::resolver::kotlin) fn namespace_receiver_candidates(
        &self,
        file: usize,
        reference: &Reference,
        receiver: &str,
        name: &str,
    ) -> Option<Vec<usize>> {
        self.imports(file, reference.context_id)
            .find(|import| imported_name(import) == Some(receiver) && import.namespace)
            .map(|import| {
                let candidates = self
                    .named(name)
                    .filter(|candidate| {
                        candidate.package == import.path && candidate.owner.is_empty()
                    })
                    .map(|candidate| candidate.candidate_index)
                    .collect();
                self.accessible(file, reference, candidates)
            })
    }

    pub(in crate::resolver::kotlin) fn imported_namespace_path_candidates(
        &self,
        file: usize,
        reference: &Reference,
        path: &[String],
    ) -> Option<Vec<usize>> {
        let name = path.last()?;
        self.imports(file, reference.context_id)
            .filter(|import| {
                import.namespace && imported_name(import) == path.first().map(String::as_str)
            })
            .find_map(|import| {
                let qualified = import
                    .path
                    .iter()
                    .chain(path.iter().skip(1))
                    .cloned()
                    .collect::<Vec<_>>();
                (1..qualified.len()).rev().find_map(|split| {
                    let candidates = self
                        .named(name)
                        .filter(|candidate| {
                            candidate.package == &qualified[..split] && candidate.owner.is_empty()
                        })
                        .map(|candidate| candidate.candidate_index)
                        .collect::<Vec<_>>();
                    (!candidates.is_empty()).then(|| self.accessible(file, reference, candidates))
                })
            })
    }

    pub(in crate::resolver::kotlin) fn imported_candidates(
        &self,
        file: usize,
        reference: &Reference,
        name: &str,
        wildcard_only: bool,
    ) -> Vec<usize> {
        self.imports(file, reference.context_id)
            .filter_map(|import| match (import.namespace, wildcard_only) {
                (true, true) => Some((import, name)),
                (false, false) if imported_name(import) == Some(name) => Some((
                    import,
                    import.path.last().map(String::as_str).unwrap_or(name),
                )),
                _ => None,
            })
            .flat_map(|(import, imported)| {
                self.named(imported)
                    .filter(move |candidate| match import.namespace {
                        true => candidate.package == import.path && candidate.owner.is_empty(),
                        false => candidate_type_segments(candidate) == import.path,
                    })
                    .filter(|candidate| !self.is_local_candidate(candidate))
                    .filter(|candidate| self.visible_in_source_set(file, candidate))
                    .map(|candidate| candidate.candidate_index)
            })
            .collect()
    }
}
