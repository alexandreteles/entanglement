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
        order: 0,
    };
    for forced in &settings.forced {
        if let Some(target) = visit.inventory.include(path, forced, true, settings) {
            visit.file(&target, Active::Yes);
        }
    }
    visit.file(path, Active::Yes);
    Unit {
        root: path.to_owned(),
        cpp: settings.cpp,
        fallback: !configured && header(path),
        files: visit.files,
    }
}

struct Traversal<'a, 's> {
    settings: &'a Settings,
    inventory: &'a mut Inventory<'s>,
    env: Environment,
    stack: BTreeSet<PathBuf>,
    once: BTreeSet<PathBuf>,
    files: BTreeMap<PathBuf, FileView>,
    order: usize,
}

impl Traversal<'_, '_> {
    fn region(&mut self, path: &Path, source: &Source, range: Range<usize>, active: Active) {
        if range.is_empty() {
            return;
        }
        let file = self.files.get_mut(path).unwrap();
        self.order += 1;
        file.regions.push(Region {
            range: range.clone(),
            active,
            order: self.order,
        });
        if active == Active::No {
            return;
        }
        let first = source
            .words
            .partition_point(|(word, _)| word.start < range.start);
        for (span, name) in source.words[first..]
            .iter()
            .take_while(|(word, _)| word.start < range.end)
        {
            if self
                .env
                .macros
                .get(name)
                .is_some_and(|value| !matches!(value, Macro::Undefined))
            {
                let replacement = Replacement {
                    range: span.clone(),
                    text: view::replacement(name, &self.env),
                };
                file.uses
                    .entry(span.start)
                    .and_modify(|old| {
                        if *old != replacement {
                            old.text = None
                        }
                    })
                    .or_insert(replacement);
            }
        }
    }

    fn file(&mut self, path: &Path, active: Active) {
        if self.once.contains(path) || !self.stack.insert(path.to_owned()) {
            return;
        }
        let source = Arc::clone(&self.inventory.sources[path]);
        self.files.entry(path.into()).or_default();
        if let Some(guard) = &source.guard {
            self.env
                .macros
                .entry(guard.clone())
                .or_insert(Macro::Undefined);
        }
        let mut state = active;
        let mut branches = Vec::new();
        let mut start = 0;
        for directive in &source.directives {
            self.region(path, &source, start..directive.range.start, state);
            start = directive.range.end;
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
        self.region(path, &source, start..source.length, state);
        self.stack.remove(path);
    }

    fn directive(&mut self, path: &Path, directive: &Directive, state: Active) {
        match directive.name.as_str() {
            "pragma" if directive.argument.trim() == "once" && state == Active::Yes => {
                self.once.insert(path.into());
            }
            "include" => {
                let spelling = includes::spelling(&directive.argument, &self.env);
                let target = spelling.and_then(|(name, quoted)| {
                    self.inventory.include(path, name, quoted, self.settings)
                });
                self.files.get_mut(path).unwrap().includes.insert(
                    directive.range.start,
                    Include {
                        range: directive.range.clone(),
                        spelling: directive.argument.clone(),
                        target: target.clone(),
                        active: state,
                        system: spelling.is_some_and(|(_, quoted)| !quoted),
                    },
                );
                if let Some(target) = target {
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
                } else if value[end..].starts_with('(') {
                    Macro::Function
                } else {
                    Macro::Defined(value[end..].trim().to_owned())
                };
                self.env.macros.insert(value[..end].into(), definition);
            }
            _ => {}
        }
    }
}
