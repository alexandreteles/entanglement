use super::*;

impl Index<'_> {
    pub(super) fn accessible(
        &self,
        file: usize,
        reference: &Reference,
        candidates: Vec<usize>,
    ) -> Vec<usize> {
        let caller = &self.sources[file];
        candidates
            .into_iter()
            .filter(|index| {
                self.candidate_accessible(file, reference, &self.candidates[*index], caller)
            })
            .collect()
    }

    pub(super) fn candidate_accessible(
        &self,
        file: usize,
        reference: &Reference,
        candidate: &Candidate,
        caller: &SourceContext,
    ) -> bool {
        if candidate.file == file && candidate.context_id != reference.context_id {
            return false;
        }
        let source = &self.sources[candidate.file];
        match candidate_visibility(candidate) {
            SemanticVisibility::Private => self.private_accessible(file, reference, candidate),
            SemanticVisibility::Protected => self.protected_accessible(file, reference, candidate),
            SemanticVisibility::Internal => {
                caller.project_root == source.project_root
                    && caller.gradle_module == source.gradle_module
                    && self.visible_in_source_set(file, candidate)
            }
            SemanticVisibility::Public | SemanticVisibility::Unknown => {
                self.module_visible(caller, source) && self.visible_in_source_set(file, candidate)
            }
        }
    }

    pub(super) fn private_accessible(
        &self,
        file: usize,
        reference: &Reference,
        candidate: &Candidate,
    ) -> bool {
        candidate.file == file
            && (candidate.owner.is_empty()
                || (reference.start_byte >= candidate.scope_start
                    && reference.start_byte < candidate.scope_end))
    }

    pub(super) fn protected_accessible(
        &self,
        file: usize,
        reference: &Reference,
        candidate: &Candidate,
    ) -> bool {
        candidate.file == file
            && candidate.owner.last().is_some_and(|owner| {
                self.candidates.iter().any(|possible_owner| {
                    possible_owner.file == file
                        && possible_owner.name == *owner
                        && possible_owner.scope_start <= reference.start_byte
                        && reference.start_byte < possible_owner.scope_end
                })
            })
    }

    pub(super) fn is_local_candidate(&self, candidate: &Candidate) -> bool {
        self.graph
            .definition(candidate.graph_id)
            .is_some_and(|definition| self.graph.is_local_definition(candidate.file, definition))
    }

    pub(super) fn module_visible(&self, caller: &SourceContext, target: &SourceContext) -> bool {
        if caller.isolated_source || target.isolated_source {
            return caller.isolated_source && target.isolated_source && caller.path == target.path;
        }
        if caller.module == target.module {
            return true;
        }
        if caller.project_root != target.project_root {
            return false;
        }
        self.visible_modules
            .get(&caller.module)
            .and_then(|sets| sets.get(&caller.source_set))
            .is_some_and(|reachable| reachable.contains(&target.gradle_module))
    }

    pub(super) fn visible_in_source_set(&self, file: usize, candidate: &Candidate) -> bool {
        let caller = &self.sources[file];
        let source = &self.sources[candidate.file];
        if caller.module != source.module {
            let Some(target) = source.source_set.as_deref() else {
                return true;
            };
            // A module's test source sets are not on another module's compile
            // classpath. Common production code is visible to all platforms;
            // platform production sets require a matching caller set.
            if target.eq_ignore_ascii_case("test") || target.to_ascii_lowercase().ends_with("test")
            {
                return false;
            }
            if target.eq_ignore_ascii_case("main") || target == "commonMain" {
                return true;
            }
            return caller.source_set.as_deref() == Some(target);
        }
        match (&caller.source_set, &source.source_set) {
            (None, _) | (_, None) => true,
            (Some(caller), Some(target)) if caller == target => true,
            (Some(caller), Some(target)) => {
                self.source_edges
                    .get(&self.sources[file].module_root)
                    .is_some_and(|edges| source_set_reaches(caller, target, edges))
                    || conventional_source_edge(caller, target)
            }
        }
    }

    pub(super) fn imported_type(
        &self,
        file: usize,
        reference: &Reference,
        name: &str,
    ) -> Option<String> {
        let candidates = self
            .imported_candidates(file, reference, name, false)
            .into_iter()
            .filter(|index| is_type_declaration(&self.candidates[*index]))
            .map(|index| &self.candidates[index])
            .collect::<Vec<_>>();
        let [candidate] = candidates.as_slice() else {
            return None;
        };
        Some(candidate_type_name(candidate))
    }

    pub(super) fn imports<'a>(
        &'a self,
        file: usize,
        context_id: usize,
    ) -> impl Iterator<Item = &'a crate::model::Import> + 'a {
        self.graph
            .imports(file)
            .filter(move |item| item.context_id == context_id)
    }

    pub(super) fn named(&self, name: &str) -> impl Iterator<Item = &Candidate<'_>> {
        self.by_name
            .get(name)
            .into_iter()
            .flatten()
            .map(|index| &self.candidates[*index])
    }
}
