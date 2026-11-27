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
            Macro::Nonzero
        } else {
            Macro::Undefined
        }
    });
    let mut visit = Traversal {
        settings,
        inventory,
        predefined: macros.clone(),
        env: Environment { macros, configured },
        origins: BTreeMap::new(),
        stack: BTreeSet::new(),
        once: BTreeSet::new(),
        files: BTreeMap::new(),
        order: 0,
    };
    for forced in &settings.forced {
        let build_source = settings
            .directory
            .join(path.file_name().unwrap_or_default());
        if let Some(target) = visit
            .inventory
            .include(&build_source, forced, true, settings)
        {
            visit.file(&target, Active::Yes);
        }
    }
    visit.file(path, Active::Yes);
    Unit {
        configured,
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
    origins: BTreeMap<String, Arc<MacroSite>>,
    predefined: BTreeMap<String, Macro>,
    stack: BTreeSet<PathBuf>,
    once: BTreeSet<PathBuf>,
    files: BTreeMap<PathBuf, FileView>,
    order: usize,
}

impl Traversal<'_, '_> {
    fn region(
        &mut self,
        path: &Path,
        source: &Source,
        range: Range<usize>,
        active: Active,
        revisited: bool,
    ) {
        if range.is_empty() {
            return;
        }
        let file = self.files.get_mut(path).unwrap();
        if active != Active::No {
            let repeated = revisited && file.position(range.start).is_some();
            file.record_uses(&source.words, &range, &self.env, &self.origins, repeated);
        }
        self.order += 1;
        file.regions.push(Region {
            range,
            active,
            order: self.order,
        });
    }

    fn file(&mut self, path: &Path, active: Active) {
        if self.once.contains(path) || !self.stack.insert(path.to_owned()) {
            return;
        }
        let source = Arc::clone(&self.inventory.sources[path]);
        let revisited = self.files.contains_key(path);
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
            self.region(
                path,
                &source,
                start..directive.range.start,
                state,
                revisited,
            );
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
        self.region(path, &source, start..source.length, state, revisited);
        self.stack.remove(path);
    }

    fn directive(&mut self, path: &Path, directive: &Directive, state: Active) {
        match directive.name.as_str() {
            "pragma" if directive.argument.trim() == "once" && state == Active::Yes => {
                self.once.insert(path.into());
            }
            "include" | "import_header" | "export_header" => {
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
                        export: directive.name == "export_header",
                        system: spelling
                            .is_some_and(|(name, quoted)| !quoted && includes::standard(name)),
                    },
                );
                if let Some(target) = target {
                    if directive.name == "include" {
                        self.file(&target, state);
                    } else {
                        let caller =
                            std::mem::replace(&mut self.env.macros, self.predefined.clone());
                        let once = std::mem::take(&mut self.once);
                        let origins = std::mem::take(&mut self.origins);
                        self.file(&target, state);
                        self.origins = origins;
                        self.once = once;
                        self.env.macros = caller;
                    }
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
                let name = &value[..end];
                if directive.name == "define" {
                    self.files.get_mut(path).unwrap().definitions.insert(
                        directive.range.start,
                        (name.to_owned(), directive.range.end),
                    );
                }
                if state == Active::Yes && directive.name == "define" {
                    self.origins.insert(
                        name.to_owned(),
                        Arc::new(MacroSite {
                            file: path.to_owned(),
                            start: directive.range.start,
                        }),
                    );
                } else {
                    self.origins.remove(name);
                }
                self.env.macros.insert(name.into(), definition);
            }
            _ => {}
        }
    }
}
