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
        if reference.kind == ReferenceKind::MemberInitializer {
            return self.initializer(context, reference);
        }
        if call.is_some_and(|call| call.receiver.is_none())
            && reference.path.len() == 1
            && let Some(local) = binding(context.store, name, at)
        {
            let Some(start) = data.lambdas.get(&local.start_byte) else {
                return unknown;
            };
            return self
                .names
                .get(name.as_str())
                .into_iter()
                .flatten()
                .find(|entry| {
                    entry.unit == context.unit
                        && entry.id.file == context.file
                        && entry.id.start_byte == *start
                        && entry.position.0 == Active::Yes
                        && call.is_some_and(|call| {
                            entry
                                .symbol
                                .parameters
                                .as_ref()
                                .is_some_and(|params| params.len() == call.arguments.len())
                        })
                })
                .map_or(unknown, |entry| {
                    (Resolution::Exact(entry.id.clone()), CallDispatch::Direct)
                });
        }
        let Some(entries) = self.names.get(name.as_str()) else {
            return if external::known(&reference.path, self.headers.get(context.unit)) {
                (Resolution::External, CallDispatch::Unknown)
            } else {
                unknown
            };
        };
        let eligible = |candidate: &&Entry<'_>| {
            let local = candidate.unit == context.unit;
            let visible = local
                && ((candidate.position.1, candidate.position.2) <= (order, at)
                    || candidate.symbol.member && candidate.def.module == reference.module)
                || !local
                    && candidate.symbol.exported
                    && self.modules.visible(data, candidate.module, at);
            let type_space = reference.kind != ReferenceKind::Type
                || data.tags.get(&at).map_or(
                    context.cpp || candidate.def.kind == DefinitionKind::TypeAlias,
                    |tag| tag == &candidate.def.kind,
                );
            visible
                && type_space
                && (reference.kind == ReferenceKind::Type)
                    == matches!(
                        candidate.def.kind,
                        DefinitionKind::Struct
                            | DefinitionKind::Union
                            | DefinitionKind::Enum
                            | DefinitionKind::TypeAlias
                    )
        };
        let candidates: Vec<_> = entries.iter().filter(eligible).collect();
        let mut selected = Vec::new();
        let field = data.receivers.get(&at).map(|name| Invocation {
            receiver: Some(name.clone()),
            ..Default::default()
        });
        for owner in overload::owners(context.store, reference, call.or(field.as_ref()), data) {
            for class in self.single_bases(context.unit, owner) {
                selected.extend(
                    candidates
                        .iter()
                        .copied()
                        .filter(|candidate| candidate.def.module.0 == class),
                );
                if !selected.is_empty() {
                    break;
                }
            }
            if !selected.is_empty() {
                break;
            }
        }
        if let Some(call) = call {
            let associated = overload::associated(data, reference, call);
            selected.extend(
                candidates
                    .iter()
                    .copied()
                    .filter(|candidate| associated.contains(&candidate.def.module.0)),
            );
            if selected.is_empty() || call.constructor {
                let constructor = overload::expand(data, &reference.path, at);
                selected.extend(candidates.iter().copied().filter(|entry| {
                    entry.def.module.0.ends_with(&constructor)
                        && entry.symbol.member
                        && entry.def.module.0.last() == Some(name)
                }));
            }
        }
        let selected = overload::select(
            selected,
            call,
            overload::receiver_const(context.store, reference, call, data),
        );
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
                            && linkage::compatible(entry, candidate, entries)
                            && candidate.position.0 != Active::No
                    })
                    .collect();
                let exact_body =
                    !bodies.is_empty() && bodies.iter().all(|body| body.position.0 == Active::Yes);
                let dynamic = self.dynamic(entry) || bodies.iter().any(|body| self.dynamic(body));
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
