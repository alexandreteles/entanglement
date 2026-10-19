use super::*;

pub(super) fn psr_routes(autoload: &Value) -> impl Iterator<Item = (&str, &str, &Value)> {
    ["psr-4", "psr-0"].into_iter().flat_map(move |kind| {
        autoload
            .get(kind)
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
            .map(move |(prefix, directories)| (kind, prefix.as_str(), directories))
    })
}

pub(super) fn autoload_matches(
    root: &Path,
    autoload: &Value,
    path: &Path,
    name: &str,
    class: bool,
) -> bool {
    if paths(autoload.get("files")).any(|entry| normalize(&root.join(entry)) == normalize(path)) {
        return true;
    }
    if !class {
        return false;
    }
    (paths(autoload.get("classmap")).any(|entry| path_matches(root, entry, path))
        && !paths(autoload.get("exclude-from-classmap"))
            .any(|pattern| path_matches(root, pattern, path)))
        || psr_routes(autoload).any(|(kind, prefix, directories)| {
            let Some(suffix) = name.strip_prefix(prefix) else {
                return false;
            };
            let relative = if kind == "psr-0" {
                let (namespace, class) = name.rsplit_once('\\').unwrap_or(("", name));
                [namespace.replace('\\', "/"), class.replace('_', "/")]
                    .into_iter()
                    .filter(|part| !part.is_empty())
                    .collect::<Vec<_>>()
                    .join("/")
            } else {
                suffix.replace('\\', "/")
            };
            paths(Some(directories)).any(|directory| {
                normalize(&root.join(directory).join(format!("{relative}.php"))) == normalize(path)
            })
        })
}

fn paths(value: Option<&Value>) -> impl Iterator<Item = &str> {
    value
        .into_iter()
        .flat_map(|value| match value {
            Value::Array(values) => values.as_slice(),
            _ => std::slice::from_ref(value),
        })
        .filter_map(Value::as_str)
}

fn path_matches(root: &Path, pattern: &str, path: &Path) -> bool {
    let pattern = pattern.trim_start_matches('/');
    if !pattern.contains('*') {
        return normalize(path).starts_with(normalize(&root.join(pattern)));
    }
    let mut builder = ignore::gitignore::GitignoreBuilder::new(root);
    builder.add_line(None, pattern).is_ok()
        && builder
            .build()
            .is_ok_and(|matcher| matcher.matched(path, false).is_ignore())
}
