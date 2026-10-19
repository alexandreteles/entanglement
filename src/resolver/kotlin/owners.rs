use super::*;

impl<'a> Index<'a> {
    /// Find candidates owned by the supplied candidate's full path.
    ///
    /// The file and semantic context must match. The caller selects
    /// declaration kinds and applies language rules.
    pub(super) fn candidates_owned_by<'b>(
        &'b self,
        owner: &Candidate<'_>,
    ) -> impl Iterator<Item = &'b Candidate<'a>> + 'b {
        let file = owner.file;
        let context_id = owner.context_id;
        let path = extended_owner(owner.owner, owner.name);
        self.candidates.iter().filter(move |candidate| {
            candidate.file == file && candidate.context_id == context_id && candidate.owner == path
        })
    }

    /// Select the smallest declaration range that contains the position.
    ///
    /// The caller supplies the accepted kinds. This query does not filter
    /// semantic contexts or require an ownership relation. The start is
    /// included and the end is excluded. Equal ranges keep candidate order.
    pub(super) fn nearest_enclosing_declaration(
        &self,
        file: usize,
        position: usize,
        kinds: &[DeclarationKind],
    ) -> Option<&Candidate<'a>> {
        self.candidates
            .iter()
            .filter(|candidate| candidate.file == file)
            .filter(|candidate| {
                candidate
                    .declaration
                    .is_some_and(|declaration| kinds.contains(&declaration.kind))
            })
            .filter(|candidate| candidate.start <= position && position < candidate.end)
            .min_by_key(|candidate| candidate.end - candidate.start)
    }
}
