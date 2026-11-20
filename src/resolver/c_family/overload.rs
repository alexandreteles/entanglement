use super::*;

pub(super) fn owners(
    store: &SemanticStore,
    reference: &Reference,
    call: Option<&Invocation>,
    data: &Data,
) -> Vec<Vec<String>> {
    let explicit = &reference.path[..reference.path.len().saturating_sub(1)];
    let mut relative = explicit.to_vec();
    if let Some(receiver) = call.and_then(|call| call.receiver.as_deref()) {
        match receiver {
            "this" => return vec![reference.module.0.clone()],
            _ => {
                let Some(ty) = binding(store, receiver, reference.start_byte)
                    .and_then(|local| local.type_name.as_deref())
                else {
                    return vec![];
                };
                relative = ty
                    .trim_start_matches("const ")
                    .trim_end_matches(['*', '&', ' '])
                    .split("::")
                    .map(str::to_owned)
                    .collect();
            }
        }
    }
    relative = expand(data, &relative, reference.start_byte);
    let mut paths: Vec<_> = (0..=reference.module.0.len())
        .rev()
        .map(|length| [&reference.module.0[..length], relative.as_slice()].concat())
        .collect();
    if explicit.is_empty() {
        paths.extend(
            data.using
                .iter()
                .filter(|(start, end, _, _)| {
                    *start < reference.start_byte && reference.start_byte < *end
                })
                .filter_map(|(_, _, path, namespace)| {
                    if *namespace {
                        Some(path.clone())
                    } else if path.last() == reference.path.last() {
                        Some(path[..path.len() - 1].to_vec())
                    } else {
                        None
                    }
                }),
        );
    }
    paths
}

pub(super) fn select<'a>(
    entries: Vec<&'a Entry<'a>>,
    call: Option<&Invocation>,
) -> Vec<&'a Entry<'a>> {
    let Some(call) = call else { return entries };
    let args = |entry: &Entry<'_>| {
        if call.operator && entry.symbol.member {
            &call.arguments[1..]
        } else {
            call.arguments.as_slice()
        }
    };
    let arity: Vec<_> = entries
        .into_iter()
        .filter(|entry| {
            entry.symbol.parameters.as_ref().is_some_and(|params| {
                entry.symbol.minimum <= args(entry).len()
                    && (entry.symbol.variadic || args(entry).len() <= params.len())
            })
        })
        .collect();
    let exact: Vec<_> = arity
        .iter()
        .copied()
        .filter(|entry| {
            let params = entry.symbol.parameters.as_deref().unwrap_or_default();
            args(entry).iter().zip(params).all(|(arg, param)| {
                arg.as_deref().is_none_or(|arg| {
                    arg == param || param.trim_start_matches("const ").trim_end_matches('&') == arg
                })
            })
        })
        .collect();
    if exact.is_empty() { arity } else { exact }
}

pub(super) fn expand(data: &Data, path: &[String], at: usize) -> Vec<String> {
    let mut path = path.to_vec();
    let mut visited = BTreeSet::new();
    while let Some(first) = path.first()
        && visited.insert(first.clone())
        && let Some((start, end, target)) = data.aliases.get(first)
        && *start < at
        && at < *end
    {
        path = [target.as_slice(), &path[1..]].concat();
    }
    path
}

pub(super) fn associated(
    data: &Data,
    reference: &Reference,
    call: &Invocation,
) -> BTreeSet<Vec<String>> {
    let mut result = BTreeSet::new();
    if call.qualified || call.receiver.is_some() {
        return result;
    }
    for (index, argument) in call.arguments.iter().enumerate() {
        let Some(ty) = argument else { continue };
        let path: Vec<_> = ty
            .trim_start_matches("const ")
            .trim_end_matches(['&', '*', ' '])
            .split("::")
            .map(str::to_owned)
            .collect();
        let mut path = expand(data, &path, reference.start_byte);
        if call.operator && index == 0 {
            result.insert(path.clone());
        }
        if path.len() > 1 {
            path.pop();
            result.insert(path);
        }
    }
    result
}
