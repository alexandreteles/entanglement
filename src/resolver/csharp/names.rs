use super::index::{Index, Site, Symbol};
use crate::languages::csharp::facts::qualify;

pub(super) fn key(name: &str) -> String {
    let name = name
        .trim()
        .trim_start_matches("global::")
        .trim_end_matches('?')
        .replace("::", ".");
    let Some(open) = name.find('<') else {
        return name;
    };
    let mut depth = 0usize;
    let mut count = 1;
    for ch in name[open + 1..].chars() {
        match ch {
            '<' => depth += 1,
            '>' if depth == 0 => break,
            '>' => depth -= 1,
            ',' if depth == 0 => count += 1,
            _ => {}
        }
    }
    format!("{}`{count}", name[..open].trim())
}

pub(super) fn primitive(name: &str) -> &str {
    match name {
        "System.Boolean" => "bool",
        "System.String" => "string",
        "System.Int32" => "int",
        "System.UInt32" => "uint",
        "System.Int64" => "long",
        "System.UInt64" => "ulong",
        "System.Byte" => "byte",
        "System.SByte" => "sbyte",
        "System.Int16" => "short",
        "System.UInt16" => "ushort",
        "System.Object" => "object",
        "System.Char" => "char",
        "System.Double" => "double",
        "System.Single" => "float",
        "System.Decimal" => "decimal",
        "System.Void" => "void",
        other => other,
    }
}
pub(super) fn builtin(name: &str) -> bool {
    matches!(
        primitive(name),
        "bool"
            | "string"
            | "int"
            | "uint"
            | "long"
            | "ulong"
            | "byte"
            | "sbyte"
            | "short"
            | "ushort"
            | "nint"
            | "nuint"
            | "object"
            | "char"
            | "double"
            | "float"
            | "decimal"
            | "void"
            | "dynamic"
    )
}
impl<'a> Index<'a> {
    pub fn named_types(&self, site: Site<'_>, name: &str) -> Vec<Symbol<'a>> {
        self.types
            .get(name)
            .into_iter()
            .flatten()
            .copied()
            .filter(|s| self.visible(site, *s))
            .collect()
    }
    pub fn type_names(&self, site: Site<'_>, name: &str) -> Vec<String> {
        let normalized = key(name);
        if builtin(&normalized) {
            return vec![primitive(&normalized).into()];
        }
        if name.starts_with("global::") {
            return vec![normalized];
        }
        let imports = self.imports(site);
        let (first, rest) = normalized.split_once('.').unwrap_or((&normalized, ""));
        let mut aliases = imports
            .iter()
            .filter(|(alias, _, _)| *alias == first)
            .map(|(_, target, _)| {
                if rest.is_empty() {
                    key(target)
                } else {
                    format!("{}.{rest}", key(target))
                }
            })
            .collect::<Vec<_>>();
        if !aliases.is_empty() {
            aliases.sort();
            aliases.dedup();
            return aliases;
        }
        for enclosing in [site.owner, site.namespace] {
            let mut prefix = enclosing;
            while !prefix.is_empty() {
                let candidate = qualify(prefix, &normalized);
                if !self.named_types(site, &candidate).is_empty() {
                    return vec![candidate];
                }
                prefix = prefix.rsplit_once('.').map_or("", |(parent, _)| parent);
            }
        }
        let mut imported = imports
            .iter()
            .filter(|(alias, _, _)| alias.is_empty())
            .map(|(_, namespace, _)| qualify(namespace, &normalized))
            .filter(|name| !self.named_types(site, name).is_empty())
            .collect::<Vec<_>>();
        if !self.named_types(site, &normalized).is_empty() {
            imported.push(normalized.clone());
        }
        if imported.is_empty() {
            return vec![normalized];
        }
        imported.sort();
        imported.dedup();
        imported
    }
    pub fn bases(&self, site: Site<'_>, owner: &str) -> Vec<String> {
        self.named_types(site, owner)
            .into_iter()
            .flat_map(|s| {
                let decl = s.decl;
                let view = self.target_view(site, s);
                let origin = Site {
                    file: s.file,
                    view,
                    at: decl.range.start,
                    namespace: &decl.namespace,
                    owner: &decl.owner,
                };
                decl.bases
                    .iter()
                    .flat_map(move |base| self.type_names(origin, base))
            })
            .collect()
    }
    pub fn target_view(&self, site: Site<'_>, symbol: Symbol<'_>) -> Option<usize> {
        let view = site.view?;
        self.project
            .owners(self.files[symbol.file].target)
            .iter()
            .copied()
            .find(|v| {
                self.project.reachable[view].contains(v)
                    && self.project.active(
                        self.files[symbol.file].target,
                        *v,
                        symbol.decl.range.start,
                    )
            })
    }
}
