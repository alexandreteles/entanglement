use super::{
    index::{Index, Site, Symbol},
    names,
};
use crate::languages::csharp::facts::Expression;

impl Index<'_> {
    pub fn inferred(&self, site: Site<'_>, value: &Expression, depth: usize) -> Vec<String> {
        if depth >= 12 {
            return Vec::new();
        }
        match value {
            Expression::Unknown => Vec::new(),
            Expression::Type(ty) => self.type_names(site, ty),
            Expression::Name(name) if name == "this" => vec![site.owner.into()],
            Expression::Name(name) if name == "base" => self.bases(site, site.owner),
            Expression::Name(name) => {
                if let Some(binding) = self.binding(site, name) {
                    return self.inferred(
                        Site {
                            at: binding.position.saturating_sub(1),
                            ..site
                        },
                        &binding.value,
                        depth + 1,
                    );
                }
                let members = self.member_named(site, site.owner, name);
                if !members.is_empty() {
                    return self.returns(site, members);
                }
                self.type_names(site, name)
            }
            Expression::Call(at) => {
                let Some((_, usage)) = self.sources[site.file]
                    .uses
                    .range((*at, 0)..=(*at, usize::MAX))
                    .next()
                else {
                    return Vec::new();
                };
                let origin = Site {
                    at: *at,
                    owner: &usage.owner,
                    namespace: &usage.namespace,
                    ..site
                };
                self.returns(origin, self.select(origin, usage, depth + 1))
            }
            Expression::Member(receiver, name) => {
                let mut result = Vec::new();
                for base in self.inferred(site, receiver, depth + 1) {
                    let members = self.member_named(site, &base, name);
                    if members.is_empty() {
                        result.push(format!("{base}.{name}"));
                    } else {
                        result.extend(self.returns(site, members));
                    }
                }
                result
            }
        }
    }

    fn returns(&self, site: Site<'_>, items: Vec<Symbol<'_>>) -> Vec<String> {
        let mut types = items
            .into_iter()
            .flat_map(|symbol| {
                let decl = symbol.decl;
                let origin = Site {
                    file: symbol.file,
                    view: self.target_view(site, symbol),
                    at: decl.range.start,
                    namespace: &decl.namespace,
                    owner: &decl.owner,
                };
                if decl.is_type {
                    vec![decl.qualified()]
                } else if decl.ty.is_empty() {
                    Vec::new()
                } else {
                    self.type_names(origin, &decl.ty)
                }
            })
            .collect::<Vec<_>>();
        types.sort();
        types.dedup();
        types
    }
    pub fn external_receiver(&self, site: Site<'_>, receiver: &Expression) -> bool {
        match receiver {
            Expression::Type(name)=>names::builtin(name)||name.starts_with("System."),
            Expression::Name(name)=>self.binding(site,name).is_some_and(|b|matches!(&b.value,Expression::Type(ty) if names::builtin(ty)||ty.starts_with("System."))) || name=="System",
            Expression::Member(inner,_)=>self.external_receiver(site,inner),
            _=>false
        }
    }
}
