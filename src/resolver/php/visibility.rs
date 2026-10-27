use super::*;

impl Index<'_> {
    pub(super) fn visible(&self, file: usize, context: usize, candidate: &Candidate<'_>) -> bool {
        let target = candidate.site.file;
        let contexts_agree = file != target || context == candidate.site.context;
        contexts_agree
            && ((self.projects.reaches(file, target)
                && self.projects.autoloads(
                    file,
                    target,
                    self.graph.file(target).target,
                    &candidate.name,
                    candidate.php.kind == PhpSymbolKind::Class,
                ))
                || self.included(file, context, target, &mut HashSet::new()))
    }

    fn included(
        &self,
        file: usize,
        context: usize,
        target: usize,
        visited: &mut HashSet<usize>,
    ) -> bool {
        if file == target {
            return true;
        }
        if !visited.insert(file) {
            return false;
        }
        self.facts(file, context)
            .into_iter()
            .flat_map(|facts| &facts.includes)
            .filter(|include| !include.conditional)
            .filter_map(|include| include.path.as_deref())
            .filter_map(|path| {
                let source = self.graph.file(file).target;
                let path = if let Some(suffix) = path.strip_prefix("@dir:") {
                    source.parent()?.join(suffix.trim_start_matches('/'))
                } else if let Some(suffix) = path.strip_prefix("@file:") {
                    std::path::PathBuf::from(format!("{}{suffix}", source.display()))
                } else {
                    source.parent()?.join(path)
                };
                let path = super::super::files::indexing::normalize(&path);
                self.graph.aliases().get(&path)
            })
            .flatten()
            .copied()
            .any(|included| included == target || self.included(included, context, target, visited))
    }
}
