use super::index::{Index, Site, Symbol};
use crate::languages::csharp::facts::{Expression, Usage};
use crate::model::ReferenceKind;
use std::collections::BTreeSet;

impl<'a> Index<'a> {
    pub fn select(&self, site: Site<'_>, usage: &Usage, depth: usize) -> Vec<Symbol<'a>> {
        if depth >= 12 {
            return Vec::new();
        }
        if usage.kind == ReferenceKind::Type || usage.constructor {
            let mut types = self
                .type_names(site, &usage.name)
                .into_iter()
                .flat_map(|n| self.named_types(site, &n))
                .collect::<Vec<_>>();
            if usage.attribute {
                let suffix = format!("{}Attribute", usage.name);
                types.extend(
                    self.type_names(site, &suffix)
                        .into_iter()
                        .flat_map(|n| self.named_types(site, &n)),
                );
            }
            if usage.constructor {
                let constructors = types
                    .iter()
                    .flat_map(|s| self.member_named(site, &s.decl.qualified(), &s.decl.name))
                    .filter(|s| s.decl.category == "constructor")
                    .collect::<Vec<_>>();
                if !constructors.is_empty() {
                    return self.best(site, usage, constructors, false, depth);
                }
                types.retain(|s| s.decl.parameters.len() == usage.arguments.len());
            }
            return self.partial(site, types);
        }
        let implicit = matches!(usage.receiver, Expression::Unknown);
        if implicit && self.binding(site, &usage.name).is_some() {
            return Vec::new();
        }
        let receiver = if implicit {
            vec![site.owner.to_owned()]
        } else {
            self.inferred(site, &usage.receiver, depth + 1)
        };
        let mut pending = receiver.clone();
        let mut seen = BTreeSet::new();
        while !pending.is_empty() {
            let level = std::mem::take(&mut pending);
            let mut options = Vec::new();
            for owner in level {
                if !seen.insert(owner.clone()) {
                    continue;
                }
                options.extend(self.member_named(site, &owner, &usage.name));
                pending.extend(self.bases(site, &owner));
            }
            let selected = self.best(site, usage, options, false, depth);
            if !selected.is_empty() {
                return selected;
            }
        }
        let imports = self.imports(site);
        if implicit {
            let options = imports
                .iter()
                .filter(|(_, _, is_static)| *is_static)
                .flat_map(|(_, ty, _)| self.member_named(site, ty, &usage.name))
                .collect();
            let selected = self.best(site, usage, options, false, depth);
            if !selected.is_empty() || usage.kind != ReferenceKind::Value {
                return selected;
            }
            return self.partial(
                site,
                self.type_names(site, &usage.name)
                    .into_iter()
                    .flat_map(|n| self.named_types(site, &n))
                    .collect(),
            );
        }
        let options = self
            .extensions
            .get(usage.name.as_str())
            .into_iter()
            .flatten()
            .copied()
            .filter(|s| {
                self.visible(site, *s)
                    && (s.decl.namespace == site.namespace
                        || imports.iter().any(|(alias, target, _)| {
                            alias.is_empty()
                                && (*target == s.decl.namespace || *target == s.decl.owner)
                        }))
                    && s.decl.extension.as_deref().is_some_and(|ty| {
                        let origin = Site {
                            file: s.file,
                            view: self.target_view(site, *s),
                            at: s.decl.range.start,
                            namespace: &s.decl.namespace,
                            owner: &s.decl.owner,
                        };
                        self.type_names(origin, ty).iter().any(|expected| {
                            receiver.iter().any(|r| {
                                r == expected
                                    || self.bases(site, r).contains(expected)
                                    || s.decl.generics.contains(expected)
                            })
                        })
                    })
            })
            .collect();
        self.best(site, usage, options, true, depth)
    }

    pub fn member_named(&self, site: Site<'_>, owner: &str, name: &str) -> Vec<Symbol<'a>> {
        self.members
            .get(owner)
            .and_then(|members| members.get(name))
            .into_iter()
            .flatten()
            .copied()
            .filter(|s| {
                let d = s.decl;
                let script =
                    crate::languages::csharp::project::extension(self.files[s.file].target, "csx");
                self.visible(site, *s)
                    && d.explicit.is_none()
                    && (d.category != "local"
                        || (s.file == site.file && d.scope.contains(&site.at))
                        || script && d.owner.is_empty())
                    && (d.visibility != "private"
                        || site.owner == d.owner
                        || site.owner.starts_with(&format!("{}.", d.owner))
                        || script)
            })
            .collect()
    }
    fn partial(&self, site: Site<'_>, mut symbols: Vec<Symbol<'a>>) -> Vec<Symbol<'a>> {
        if symbols.len() > 1
            && symbols.iter().all(|s| {
                self.target_view(site, *s) == self.target_view(site, symbols[0])
                    && s.decl.modifiers.contains("partial")
                    && s.decl.qualified() == symbols[0].decl.qualified()
            })
        {
            symbols.truncate(1);
        }
        symbols
    }
}
