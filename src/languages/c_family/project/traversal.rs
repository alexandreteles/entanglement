use super::*;

pub(super) fn unit(
    path: &Path,
    settings: &Settings,
    configured: bool,
    inventory: &mut Inventory<'_>,
) -> Unit {
    let mut macros: BTreeMap<_, _> = settings
        .macros
        .iter()
        .map(|(name, value)| {
            (
                name.clone(),
                value.clone().map_or(Macro::Undefined, Macro::Defined),
            )
        })
        .collect();
    macros.entry("__cplusplus".into()).or_insert_with(|| {
        if settings.cpp {
            Macro::Defined("201703".into())
        } else {
            Macro::Undefined
        }
    });
    let mut visit = Traversal {
        settings,
        inventory,
        env: Environment { macros, configured },
        stack: BTreeSet::new(),
        once: BTreeSet::new(),
        files: BTreeMap::new(),
    };
    for forced in &settings.forced {
        if let Some(target) = visit.inventory.include(path, forced, true, settings) {
            visit.file(&target, Active::Yes);
        }
    }
    visit.file(path, Active::Yes);
    Unit {
        cpp: settings.cpp,
        files: visit.files,
    }
}

struct Traversal<'a, 's> {
    settings: &'a Settings,
    inventory: &'a mut Inventory<'s>,
    env: Environment,
    stack: BTreeSet<PathBuf>,
    once: BTreeSet<PathBuf>,
    files: BTreeMap<PathBuf, Active>,
}

impl Traversal<'_, '_> {
    fn file(&mut self, path: &Path, active: Active) {
        if self.once.contains(path) || !self.stack.insert(path.to_owned()) {
            return;
        }
        let source = Arc::clone(&self.inventory.sources[path]);
        self.files
            .entry(path.into())
            .and_modify(|state| *state = state.or(active))
            .or_insert(active);
        if let Some(guard) = &source.guard {
            self.env
                .macros
                .entry(guard.clone())
                .or_insert(Macro::Undefined);
        }
        let mut state = active;
        let mut branches = Vec::new();
        for directive in &source.directives {
            let name = directive.name.as_str();
            match name {
                "if" | "ifdef" | "ifndef" => {
                    let test = self.env.condition(name, &directive.argument);
                    branches.push((state, test));
                    state = state.and(test);
                }
                "elif" | "elifdef" | "elifndef" | "else" => {
                    if let Some((parent, seen)) = branches.last_mut() {
                        let test = if name == "else" {
                            Active::Yes
                        } else {
                            self.env.condition(name, &directive.argument)
                        };
                        state = parent.and(seen.inverse()).and(test);
                        *seen = seen.or(test);
                    }
                }
                "endif" => {
                    if let Some((parent, _)) = branches.pop() {
                        state = parent;
                    }
                }
                _ if state != Active::No => self.directive(path, directive, state),
                _ => {}
            }
        }
        self.stack.remove(path);
    }

    fn directive(&mut self, path: &Path, directive: &Directive, state: Active) {
        match directive.name.as_str() {
            "pragma" if directive.argument.trim() == "once" && state == Active::Yes => {
                self.once.insert(path.into());
            }
            "include" => {
                if let Some((name, quoted)) = includes::spelling(&directive.argument, &self.env)
                    && let Some(target) = self.inventory.include(path, name, quoted, self.settings)
                {
                    self.file(&target, state);
                }
            }
            "define" | "undef" => {
                let value = directive.argument.trim();
                let end = value
                    .find(|ch: char| !(ch.is_alphanumeric() || ch == '_'))
                    .unwrap_or(value.len());
                let definition = if state == Active::Maybe {
                    Macro::Unknown
                } else if directive.name == "undef" {
                    Macro::Undefined
                } else {
                    Macro::Defined(value[end..].trim().to_owned())
                };
                self.env.macros.insert(value[..end].into(), definition);
            }
            _ => {}
        }
    }
}
