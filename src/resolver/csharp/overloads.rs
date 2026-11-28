use super::index::{Index, Site, Symbol};
use super::names;
use crate::languages::csharp::facts::{Decl, Usage};
use crate::model::ReferenceKind;
use std::collections::BTreeSet;

impl<'a> Index<'a> {
    pub fn best(
        &self,
        site: Site<'_>,
        usage: &Usage,
        options: Vec<Symbol<'a>>,
        extension: bool,
        depth: usize,
    ) -> Vec<Symbol<'a>> {
        if usage.kind != ReferenceKind::Call && usage.name != "this" {
            return options;
        }
        if options.is_empty() {
            return options;
        }
        let values = usage
            .arguments
            .iter()
            .map(|a| self.inferred(site, &a.value, depth + 1))
            .collect::<Vec<_>>();
        let mut ranked = options
            .into_iter()
            .filter(|s| {
                matches!(
                    s.decl.category.as_str(),
                    "method" | "local" | "constructor" | "operator" | "indexer"
                )
            })
            .filter_map(|s| {
                let origin = Site {
                    file: s.file,
                    view: self.target_view(site, s),
                    at: s.decl.range.start,
                    namespace: &s.decl.namespace,
                    owner: &s.decl.owner,
                };
                self.score(origin, usage, s.decl, &values, extension)
                    .map(|score| (score, s))
            })
            .collect::<Vec<_>>();
        let Some(best) = ranked.iter().map(|(s, _)| *s).min() else {
            return Vec::new();
        };
        ranked.retain(|(s, _)| *s == best);
        let mut symbols = ranked.into_iter().map(|(_, s)| s).collect::<Vec<_>>();
        if symbols.len() > 1
            && symbols.iter().all(|s| {
                s.decl.modifiers.contains("partial")
                    && s.decl.signature() == symbols[0].decl.signature()
            })
        {
            symbols.retain(|s| s.decl.body.is_some());
        }
        symbols
    }

    fn score(
        &self,
        site: Site<'_>,
        usage: &Usage,
        decl: &Decl,
        values: &[Vec<String>],
        extension: bool,
    ) -> Option<(usize, usize, usize)> {
        if usage.arity.is_some_and(|n| n != decl.generics.len()) {
            return None;
        }
        let offset = usize::from(
            extension
                && decl
                    .parameters
                    .first()
                    .is_some_and(|p| Some(&p.ty) == decl.extension.as_ref()),
        );
        let parameters = &decl.parameters[offset..];
        let mut used = BTreeSet::new();
        let mut cost = 0;
        for (i, arg) in usage.arguments.iter().enumerate() {
            let index = match &arg.name {
                Some(name) => parameters.iter().position(|p| p.name == *name)?,
                None if i < parameters.len() => i,
                None if parameters.last().is_some_and(|p| p.variadic) => parameters.len() - 1,
                _ => return None,
            };
            let p = &parameters[index];
            if !used.insert(index) && !p.variadic
                || arg.mode != p.mode && !(arg.mode.is_empty() && p.mode == "in")
            {
                return None;
            }
            let expected = if p.variadic {
                p.ty.trim_end_matches("[]")
            } else {
                &p.ty
            };
            let candidates = self.type_names(site, expected);
            let known = &values[i];
            let rank = if known.is_empty() {
                3
            } else if known.iter().any(|ty| candidates.contains(ty)) {
                0
            } else if decl.generics.iter().any(|g| g == expected) {
                1
            } else if expected == "object"
                || !names::builtin(expected)
                    && known.iter().any(|ty| {
                        ty == "null" || self.bases(site, ty).iter().any(|b| candidates.contains(b))
                    })
            {
                2
            } else if known.iter().any(|ty| numeric(ty, expected)) {
                1
            } else {
                return None;
            };
            cost += rank;
        }
        if parameters
            .iter()
            .enumerate()
            .any(|(i, p)| !used.contains(&i) && !p.optional && !p.variadic)
        {
            return None;
        }
        Some((
            cost,
            usize::from(parameters.last().is_some_and(|p| p.variadic)),
            parameters.len() - used.len(),
        ))
    }
}

fn numeric(from: &str, to: &str) -> bool {
    match from {
        "byte" => matches!(
            to,
            "short" | "ushort" | "int" | "uint" | "long" | "ulong" | "float" | "double" | "decimal"
        ),
        "sbyte" | "short" => matches!(to, "int" | "long" | "float" | "double" | "decimal"),
        "ushort" | "char" => matches!(
            to,
            "int" | "uint" | "long" | "ulong" | "float" | "double" | "decimal"
        ),
        "int" => matches!(to, "long" | "float" | "double" | "decimal"),
        "uint" => matches!(to, "long" | "ulong" | "float" | "double" | "decimal"),
        "long" | "ulong" => matches!(to, "float" | "double" | "decimal"),
        "float" => to == "double",
        _ => false,
    }
}
