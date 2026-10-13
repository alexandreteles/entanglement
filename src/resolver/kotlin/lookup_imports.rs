use super::*;

impl Index<'_> {
    pub(super) fn namespace_receiver_candidates(
        &self,
        file: usize,
        reference: &Reference,
        receiver: &str,
        name: &str,
    ) -> Option<Vec<usize>> {
        for import in self.imports(file, reference.context_id) {
            if imported_name(import) == Some(receiver) && import.namespace {
                let candidates = self
                    .named(name)
                    .filter(|candidate| {
                        candidate.package == import.path && candidate.owner.is_empty()
                    })
                    .map(|candidate| candidate.candidate_index)
                    .collect();
                return Some(self.accessible(file, reference, candidates));
            }
        }
        None
    }
    pub(super) fn imported_namespace_path_candidates(
        &self,
        file: usize,
        reference: &Reference,
        path: &[String],
    ) -> Option<Vec<usize>> {
        let name = path.last()?;
        for import in self.imports(file, reference.context_id) {
            if import.namespace && imported_name(import) == path.first().map(String::as_str) {
                let mut qualified = import.path.clone();
                qualified.extend(path.iter().skip(1).cloned());
                for split in (1..qualified.len()).rev() {
                    let candidates = self
                        .named(name)
                        .filter(|candidate| candidate.package == qualified[..split])
                        .filter(|candidate| candidate.owner.is_empty())
                        .collect::<Vec<_>>();
                    if !candidates.is_empty() {
                        return Some(
                            self.accessible(
                                file,
                                reference,
                                candidates
                                    .iter()
                                    .map(|candidate| candidate.candidate_index)
                                    .collect(),
                            ),
                        );
                    }
                }
            }
        }
        None
    }
    pub(super) fn imported_candidates(
        &self,
        file: usize,
        reference: &Reference,
        name: &str,
        wildcard_only: bool,
    ) -> Vec<usize> {
        let mut result = Vec::new();
        for import in self.imports(file, reference.context_id) {
            if import.namespace {
                if !wildcard_only {
                    continue;
                }
                result.extend(
                    self.named(name)
                        .filter(|candidate| candidate.package == import.path)
                        .filter(|candidate| candidate.owner.is_empty())
                        .filter(|candidate| !self.is_local_candidate(candidate))
                        .filter(|candidate| self.visible_in_source_set(file, candidate))
                        .map(|candidate| candidate.candidate_index),
                );
                continue;
            }
            if wildcard_only {
                continue;
            }
            let local_name = import
                .alias
                .as_deref()
                .or_else(|| import.path.last().map(String::as_str));
            if local_name != Some(name) {
                continue;
            }
            let imported = import.path.last().map(String::as_str).unwrap_or(name);
            result.extend(
                self.named(imported)
                    .filter(|candidate| {
                        candidate_type_segments(candidate) == import.path
                            && !self.is_local_candidate(candidate)
                    })
                    .filter(|candidate| self.visible_in_source_set(file, candidate))
                    .map(|candidate| candidate.candidate_index),
            );
        }
        result
    }
}
