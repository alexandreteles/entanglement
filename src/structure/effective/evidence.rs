use super::*;

impl Analysis<'_> {
    pub(super) fn evidence(&self, unit: &str) -> Vec<Evidence> {
        let mut evidence = declaration_evidence(unit, self.facts);
        let mut visited = BTreeSet::from([unit.to_owned()]);
        let mut pending = vec![unit.to_owned()];
        while let Some(source) = pending.pop() {
            for edge in self.snapshot.relationships.iter().filter(|edge| {
                edge.source == source
                    && edge.kind == RelationshipKind::Type
                    && edge.resolution == ResolutionStatus::Resolved
            }) {
                if self
                    .declarations
                    .get(&edge.target)
                    .is_some_and(|target| target.record.alias.is_some())
                    && visited.insert(edge.target.clone())
                {
                    evidence.extend(declaration_evidence(&edge.target, self.facts));
                    pending.push(edge.target.clone());
                }
            }
        }
        evidence.sort();
        evidence.dedup();
        evidence
    }
}

pub(super) fn declaration_evidence(unit: &str, facts: &[FileFacts]) -> Vec<Evidence> {
    facts
        .iter()
        .flat_map(|fact| {
            fact.contexts.iter().flat_map(move |store| {
                store
                    .semantic
                    .structure
                    .iter()
                    .filter(move |record| {
                        record.is_type && extract::type_id(fact, store, record) == unit
                    })
                    .map(move |record| Evidence {
                        path: fact.analysis.path.clone(),
                        start: record.start,
                        end: record.end,
                        owner: unit.to_owned(),
                        detail: record.kind.clone(),
                    })
            })
        })
        .collect()
}
