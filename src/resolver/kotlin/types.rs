use super::*;

impl Index<'_> {
    /// Return Kotlin type declarations in name-index order, with duplicates.
    ///
    /// Callers select the package, imports, visibility, and ambiguity rules.
    pub(super) fn named_types(&self, name: &str) -> impl Iterator<Item = &Candidate<'_>> {
        self.named(name)
            .filter(|candidate| is_type_declaration(candidate))
    }

    /// Return top-level types in the exact package, in name-index order.
    ///
    /// Keep duplicates. Do not apply visibility or import rules.
    pub(super) fn package_types<'b>(
        &'b self,
        name: &str,
        package: &[String],
    ) -> impl Iterator<Item = &'b Candidate<'b>> {
        self.named_types(name)
            .filter(move |candidate| candidate.package == package && candidate.owner.is_empty())
    }

    /// Return types with the exact package, owner, and name segments.
    ///
    /// Keep name-index order and duplicates. Do not apply visibility rules.
    pub(super) fn qualified_types<'b>(
        &'b self,
        name: &str,
        path: &[String],
    ) -> impl Iterator<Item = &'b Candidate<'b>> {
        self.named_types(name)
            .filter(move |candidate| candidate_type_segments(candidate) == path)
    }
}
