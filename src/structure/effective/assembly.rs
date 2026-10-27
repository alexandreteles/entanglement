use super::*;

impl Analysis<'_> {
    pub(super) fn declaration(
        &self,
        unit: &str,
        declaration: &Declaration<'_>,
    ) -> EffectiveDeclaration {
        if self.ambiguous.contains(unit) {
            return EffectiveDeclaration {
                unit: unit.to_owned(),
                evidence: declaration_evidence(unit, self.facts),
                status: CoverageStatus::Unavailable,
                members: Vec::new(),
                types: Vec::new(),
                obligations: Vec::new(),
                conflicts: vec!["identity_ambiguous".to_owned()],
                satisfies: Vec::new(),
            };
        }
        let policy = adapters::Policy::for_declaration(
            declaration
                .store
                .semantic
                .structure_language
                .as_deref()
                .unwrap_or(""),
            &declaration.record.kind,
        );
        let mut obligations = self
            .snapshot
            .relationships
            .iter()
            .filter(|edge| {
                edge.source == unit
                    && matches!(
                        edge.kind,
                        RelationshipKind::Inherit
                            | RelationshipKind::Compose
                            | RelationshipKind::Implement
                    )
                    && edge.resolution != ResolutionStatus::Resolved
            })
            .map(|edge| format!("unresolved:{}", edge.target))
            .collect::<Vec<_>>();
        let paths = provenance(unit, &self.parents);
        obligations.extend(
            paths
                .keys()
                .filter(|origin| self.ambiguous.contains(*origin))
                .map(|origin| format!("ambiguous_origin:{origin}")),
        );
        let mut conflicts = Vec::new();
        let order = policy
            .order(unit, &self.parents, &paths)
            .unwrap_or_else(|error| {
                conflicts.push(error);
                vec![unit.to_owned()]
            });
        let selected = self
            .selections
            .iter()
            .filter(|selection| {
                symbol_unit(&selection.owner, &self.declarations).as_deref() == Some(unit)
            })
            .map(|selection| (selection.name.clone(), selection.origins.as_slice()))
            .collect::<BTreeMap<_, _>>();
        let candidates = candidates(&order, &paths, &self.direct);
        let mut members = self.select_members(unit, candidates, &selected, &policy, &order);
        self.add_aliases(unit, &selected, &mut members);
        obligations.extend(self.apply_selection(unit, &mut members));
        conflicts.extend(policy.conflicts(&mut members));
        obligations.extend(policy.finish(&mut members, self.snapshot));
        let mut types: Vec<TypeStructure> = declaration
            .record
            .types
            .iter()
            .map(|written| TypeStructure {
                written: written.clone(),
                expanded: expand_alias(
                    written,
                    unit,
                    &self.declarations,
                    self.snapshot,
                    &mut BTreeSet::new(),
                ),
                origin: unit.to_owned(),
            })
            .chain(
                declaration
                    .record
                    .alias
                    .iter()
                    .map(|written| TypeStructure {
                        written: written.clone(),
                        expanded: None,
                        origin: unit.to_owned(),
                    }),
            )
            .chain(
                members
                    .iter()
                    .flat_map(|member| member_types(&member.origin, self.facts)),
            )
            .collect();
        for structure in &mut types {
            if structure.expanded.is_some() {
                continue;
            }
            let source = self
                .direct
                .iter()
                .find(|(_, members)| {
                    members
                        .iter()
                        .any(|member| member.origin == structure.origin)
                })
                .map_or(unit, |(source, _)| source);
            structure.expanded = expand_alias(
                &structure.written,
                source,
                &self.declarations,
                self.snapshot,
                &mut BTreeSet::new(),
            );
        }
        members
            .sort_by(|left, right| (&left.name, &left.origin).cmp(&(&right.name, &right.origin)));
        obligations.sort();
        obligations.dedup();
        conflicts.sort();
        conflicts.dedup();
        EffectiveDeclaration {
            unit: unit.to_owned(),
            evidence: self.evidence(unit),
            status: if declaration.store.semantic.structure_errors {
                CoverageStatus::Unavailable
            } else {
                CoverageStatus::Partial
            },
            members,
            types,
            obligations,
            conflicts,
            satisfies: Vec::new(),
        }
    }
}
