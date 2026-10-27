use super::*;

impl Analysis<'_> {
    pub(super) fn select_members(
        &self,
        unit: &str,
        candidates: BTreeMap<(String, Option<String>), Vec<EffectiveMember>>,
        selected: &BTreeMap<String, &[SymbolId]>,
        policy: &adapters::Policy,
        order: &[String],
    ) -> Vec<EffectiveMember> {
        let own = self.direct.get(unit).map(Vec::as_slice).unwrap_or_default();
        let mut members = Vec::new();
        for ((name, _), mut choices) in candidates {
            choices.sort_by(|left, right| {
                (left.provenance.len(), &left.origin).cmp(&(right.provenance.len(), &right.origin))
            });
            choices.dedup_by(|left, right| left.origin == right.origin);
            let contracts = policy.inherited_contracts(&choices);
            let chosen = if let Some(origins) = selected.get(&name) {
                let ids = origins
                    .iter()
                    .filter_map(|symbol| symbol_member(symbol, self.facts))
                    .collect::<BTreeSet<_>>();
                choices
                    .into_iter()
                    .filter(|choice| ids.contains(&choice.origin))
                    .collect::<Vec<_>>()
            } else {
                policy.choose(choices, own, &name, order)
            };
            members.extend(chosen.into_iter().map(|mut member| {
                member.contracts.extend(contracts.iter().cloned());
                member.contracts.sort();
                member.contracts.dedup();
                member
            }));
        }
        members
    }

    pub(super) fn apply_selection(
        &self,
        unit: &str,
        members: &mut [EffectiveMember],
    ) -> Vec<String> {
        let mut obligations = Vec::new();
        for selection in self.selections.iter().filter(|selection| {
            symbol_unit(&selection.owner, &self.declarations).as_deref() == Some(unit)
        }) {
            obligations.extend(selection.obligations.iter().cloned());
            for (symbol, visibility) in &selection.visibility {
                let Some(origin) = symbol_member(symbol, self.facts) else {
                    continue;
                };
                for member in members
                    .iter_mut()
                    .filter(|member| member.name == selection.name && member.origin == origin)
                {
                    member.visibility = visibility.clone();
                    if !selection.obligations.is_empty()
                        && member
                            .evidence
                            .iter()
                            .any(|evidence| evidence.detail == "property")
                    {
                        member.body = None;
                        member.selection = Some("candidate".to_owned());
                    }
                }
            }
        }
        obligations
    }

    pub(super) fn add_aliases(
        &self,
        unit: &str,
        selected: &BTreeMap<String, &[SymbolId]>,
        members: &mut Vec<EffectiveMember>,
    ) {
        for (name, origins) in selected {
            if members.iter().any(|member| member.name == *name) {
                continue;
            }
            for symbol in *origins {
                if let Some(origin) = symbol_member(symbol, self.facts)
                    && let Some(original) = self
                        .direct
                        .values()
                        .flatten()
                        .find(|member| member.origin == origin)
                {
                    let mut member = original.clone();
                    member.name = name.clone();
                    member.provenance = vec![unit.to_owned(), origin];
                    members.push(member);
                }
            }
        }
    }
}

pub(super) fn candidates(
    order: &[String],
    paths: &BTreeMap<String, Vec<String>>,
    direct: &BTreeMap<String, Vec<EffectiveMember>>,
) -> BTreeMap<(String, Option<String>), Vec<EffectiveMember>> {
    let mut candidates = BTreeMap::<(String, Option<String>), Vec<EffectiveMember>>::new();
    for origin in order {
        for member in direct.get(origin).into_iter().flatten() {
            let mut member = member.clone();
            member.provenance = paths
                .get(origin)
                .cloned()
                .unwrap_or_else(|| vec![origin.clone()]);
            candidates
                .entry((member.name.clone(), member.member_kind.clone()))
                .or_default()
                .push(member);
        }
    }
    candidates
}
