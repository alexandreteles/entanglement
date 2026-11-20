use super::*;

impl Analysis<'_> {
    /// Count distinct base subobjects. A virtual base has one identity for the
    /// most-derived class. Two identities suffice to establish ambiguity.
    pub(super) fn cpp_members(
        &self,
        unit: &str,
        members: &mut Vec<EffectiveMember>,
    ) -> Vec<String> {
        let mut instances = BTreeMap::<&str, BTreeSet<usize>>::new();
        let mut shared = BTreeMap::new();
        let mut next = 1;
        let mut pending = VecDeque::from([(unit, 0)]);
        while let Some((class, id)) = pending.pop_front() {
            let seen = instances.entry(class).or_default();
            if seen.len() == 2 || !seen.insert(id) {
                continue;
            }
            for base in self.parents.get(class).into_iter().flatten() {
                let identity = if self.virtual_bases.contains(&(class, base.as_str())) {
                    *shared.entry(base).or_insert_with(|| {
                        next += 1;
                        next
                    })
                } else {
                    next += 1;
                    next
                };
                pending.push_back((base, identity));
            }
        }
        members.retain(|member| member.visibility != "private" || member.provenance.len() == 1);
        let mut conflicts = Vec::new();
        for member in members {
            if member
                .member_kind
                .as_deref()
                .is_some_and(|kind| kind.starts_with("static_"))
            {
                continue;
            }
            if member
                .provenance
                .last()
                .and_then(|class| instances.get(class.as_str()))
                .is_some_and(|ids| ids.len() > 1)
            {
                member.selection = Some("ambiguous".into());
                member.body = None;
                conflicts.push(format!("ambiguous_base:{}", member.name));
            }
        }
        conflicts
    }
}
