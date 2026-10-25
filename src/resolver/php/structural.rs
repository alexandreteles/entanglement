use super::*;

#[derive(Default)]
pub(super) struct MemberCandidates {
    pub members: Vec<Member>,
    pub version_dependent: bool,
}

impl Index<'_> {
    /// Retain property and constant alternatives when trait precedence depends on PHP.
    /// Method selection continues to use the existing method rules.
    pub(super) fn structural_member_candidates(
        &self,
        class: usize,
        kind: PhpSymbolKind,
        name: &str,
    ) -> MemberCandidates {
        if kind == PhpSymbolKind::Method {
            return MemberCandidates {
                members: self.type_members(class, kind, name, &mut HashSet::new()),
                version_dependent: false,
            };
        }
        self.composed_member_candidates(class, kind, name, &mut HashSet::new())
    }

    fn composed_member_candidates(
        &self,
        class: usize,
        kind: PhpSymbolKind,
        name: &str,
        visited: &mut HashSet<usize>,
    ) -> MemberCandidates {
        if !visited.insert(class) {
            return MemberCandidates::default();
        }
        let selected = self.type_members(class, kind, name, &mut HashSet::new());
        if selected
            .iter()
            .any(|member| self.owns_member(class, member))
        {
            return MemberCandidates {
                members: selected,
                version_dependent: false,
            };
        }
        let traits = merge_candidates(self.traits(class).into_iter().map(|parent| {
            self.composed_member_candidates(parent, kind, name, &mut visited.clone())
        }));
        let parents = self.parents(class);
        let concrete = parents
            .iter()
            .any(|parent| !self.candidates[*parent].php.is_interface);
        let mut parents = merge_candidates(
            parents
                .into_iter()
                .filter(|parent| !concrete || !self.candidates[*parent].php.is_interface)
                .map(|parent| {
                    self.composed_member_candidates(parent, kind, name, &mut visited.clone())
                }),
        );
        if traits.members.is_empty() {
            return parents;
        }
        parents.members.retain(|member| {
            let origin = &self.candidates[member.candidate];
            origin.declaration.visibility != SemanticVisibility::Private
                && member
                    .class
                    .is_none_or(|owner| !self.candidates[owner].php.is_interface)
        });
        if parents.members.is_empty() {
            return traits;
        }
        let mut candidates = merge_candidates([traits, parents]);
        candidates.version_dependent = true;
        candidates
    }

    fn owns_member(&self, class: usize, member: &Member) -> bool {
        let owner = &self.candidates[class];
        let member = &self.candidates[member.candidate];
        member
            .owner
            .is_some_and(|name| name.eq_ignore_ascii_case(&owner.name))
            && member.site.file == owner.site.file
            && member.site.context == owner.site.context
    }
}

fn merge_candidates(candidates: impl IntoIterator<Item = MemberCandidates>) -> MemberCandidates {
    let mut result = MemberCandidates::default();
    for candidates in candidates {
        result.version_dependent |= candidates.version_dependent;
        result.members.extend(candidates.members);
    }
    result.members.sort_by_key(|member| member.candidate);
    result.members.dedup_by_key(|member| member.candidate);
    result
}
