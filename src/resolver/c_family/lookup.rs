use super::*;

impl<'a> Index<'a> {
    pub(super) fn lookup(&self, context: &Context<'_>, reference: &Reference) -> Outcome {
        let unknown = (Resolution::Unresolved, CallDispatch::Unknown);
        let Some((active, order, at)) = position(context.view, reference.start_byte) else {
            return unknown;
        };
        let Some(data) = context.store.semantic.c_family.as_deref() else {
            return unknown;
        };
        let Some(name) = reference.path.last() else {
            return unknown;
        };
        let call = data.calls.get(&reference.start_byte);
        if active != Active::Yes
            || context.view.is_some_and(|view| view.uses.contains_key(&at))
            || call.is_some_and(|call| call.indirect)
        {
            return unknown;
        }
        if call.is_some_and(|call| call.receiver.is_none())
            && reference.path.len() == 1
            && binding(context.store, name, at).is_some()
        {
            return unknown;
        }
        let Some(entries) = self.names.get(name.as_str()) else {
            return unknown;
        };
        let paths = overload::owners(context.store, reference, call, data);
        let mut selected = Vec::new();
        for owner in paths {
            selected = entries
                .iter()
                .filter(|candidate| {
                    let local = candidate.unit == context.unit;
                    let visible = local
                        && ((candidate.position.1, candidate.position.2) <= (order, at)
                            || candidate.symbol.member && candidate.def.module == reference.module)
                        || !local
                            && candidate.symbol.exported
                            && self.modules.visible(data, candidate.module, at);
                    visible
                        && candidate.def.module.0 == owner
                        && (reference.kind == ReferenceKind::Type)
                            == matches!(
                                candidate.def.kind,
                                DefinitionKind::Struct
                                    | DefinitionKind::Union
                                    | DefinitionKind::Enum
                                    | DefinitionKind::TypeAlias
                            )
                })
                .collect();
            if !selected.is_empty() {
                break;
            }
        }
        let selected = overload::select(selected, call);
        if selected.is_empty() {
            return unknown;
        }
        let mut answers = Vec::new();
        for entry in selected {
            if entry.position.0 != Active::Yes {
                answers.push(unknown.clone());
                continue;
            }
            if reference.kind == ReferenceKind::Type {
                let complete = entries.iter().filter(|candidate| {
                    candidate.unit == entry.unit
                        && candidate.def.module == entry.def.module
                        && candidate.def.kind == entry.def.kind
                        && candidate.symbol.body
                        && candidate.position.0 == Active::Yes
                });
                let targets: BTreeSet<_> = complete.map(|candidate| candidate.id.clone()).collect();
                answers.push((target(targets, &entry.id), CallDispatch::Unknown));
            } else {
                let bodies: Vec<_> = entries
                    .iter()
                    .filter(|candidate| {
                        candidate.symbol.body
                            && compatible(entry, candidate, entries)
                            && candidate.position.0 != Active::No
                    })
                    .collect();
                let exact_body =
                    !bodies.is_empty() && bodies.iter().all(|body| body.position.0 == Active::Yes);
                let dynamic = entry.symbol.dynamic || bodies.iter().any(|body| body.symbol.dynamic);
                let dispatch = if !exact_body {
                    CallDispatch::Unknown
                } else if dynamic && call.is_none_or(|call| !call.qualified) {
                    CallDispatch::Virtual
                } else {
                    CallDispatch::Direct
                };
                let ids = bodies
                    .iter()
                    .map(|candidate| candidate.id.clone())
                    .collect();
                answers.push((target(ids, &entry.id), dispatch));
            }
        }
        combine(answers)
    }
}

fn target(mut symbols: BTreeSet<SymbolId>, fallback: &SymbolId) -> Resolution {
    match symbols.len() {
        0 => Resolution::Exact(fallback.clone()),
        1 => Resolution::Exact(symbols.pop_first().unwrap()),
        _ => Resolution::Ambiguous(symbols.into_iter().collect()),
    }
}

fn compatible(left: &Entry<'_>, right: &Entry<'_>, entries: &[Entry<'_>]) -> bool {
    if left.def.module != right.def.module
        || left.symbol.parameters != right.symbol.parameters
        || left.symbol.qualifiers != right.symbol.qualifiers
        || left.module != right.module
    {
        return false;
    }
    if left.symbol.result.is_some()
        && right.symbol.result.is_some()
        && left.symbol.result != right.symbol.result
    {
        return false;
    }
    if left.symbol.internal || right.symbol.internal {
        return left.unit == right.unit;
    }
    if left.cpp == right.cpp {
        return true;
    }
    let c_linkage = |entry: &Entry<'_>| {
        !entry.cpp
            || entry.symbol.c_linkage
            || entries.iter().any(|declaration| {
                declaration.unit == entry.unit
                    && declaration.symbol.c_linkage
                    && declaration.def.module == entry.def.module
                    && declaration.symbol.parameters == entry.symbol.parameters
                    && declaration.position.1 <= entry.position.1
            })
    };
    c_linkage(left) && c_linkage(right)
}
