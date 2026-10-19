use super::*;

impl Index<'_> {
    pub(in crate::resolver::php) fn receiver_classes(
        &self,
        file: usize,
        reference: &Reference,
        php: &PhpReference,
        receiver: &str,
        seen: &mut HashSet<usize>,
    ) -> Option<Vec<usize>> {
        if !seen.insert(php.start_byte) {
            return None;
        }
        let site = Site::at(file, reference);
        match php.receiver_reference {
            Some(start) => Some(self.typed_receiver(file, reference, start, seen)),
            None if ["$this", "self", "parent", "static"]
                .iter()
                .any(|name| receiver.eq_ignore_ascii_case(name)) =>
            {
                Some(self.lookup(
                    site,
                    match receiver.eq_ignore_ascii_case("$this") {
                        true => "self",
                        false => receiver,
                    },
                    SymbolKind::Class,
                    php.class.as_deref(),
                ))
            }
            None => {
                let name = match receiver.strip_prefix("new ") {
                    Some(name) => name,
                    None if receiver.starts_with('$') => self
                        .receiver_binding(file, reference, receiver)
                        .filter(|binding| binding.immutable)?
                        .type_name
                        .as_deref()?
                        .trim_start_matches('?'),
                    None => receiver,
                };
                Some(self.lookup(site, name, SymbolKind::Class, None))
            }
        }
    }

    pub(in crate::resolver::php) fn typed_receiver(
        &self,
        file: usize,
        reference: &Reference,
        start: usize,
        seen: &mut HashSet<usize>,
    ) -> Vec<usize> {
        let Some(inner) =
            self.graph.file(file).references().find(|inner| {
                inner.start_byte == start && inner.context_id == reference.context_id
            })
        else {
            return Vec::new();
        };
        let Some(php) = self
            .facts(file, reference.context_id)
            .and_then(|facts| facts.references.iter().find(|php| php.start_byte == start))
        else {
            return Vec::new();
        };
        let candidates = match (php.kind, php.callable) {
            (RefKind::Function, true) => self.lookup(
                Site::at(file, inner),
                &php.name,
                SymbolKind::Function,
                php.class.as_deref(),
            ),
            (RefKind::Method | RefKind::StaticMethod, true)
            | (RefKind::Property | RefKind::StaticProperty, _) => self
                .member_reference(file, inner, php, seen)
                .into_iter()
                .map(|member| member.candidate)
                .collect(),
            _ => return Vec::new(),
        };
        let [candidate] = candidates.as_slice() else {
            return Vec::new();
        };
        let candidate = &self.candidates[*candidate];
        if candidate.php.conditional {
            return Vec::new();
        }
        let type_name = match candidate.php.kind {
            SymbolKind::Property => candidate.declaration.declared_type.as_deref(),
            _ => candidate
                .declaration
                .signature
                .as_ref()
                .and_then(|signature| signature.return_type.as_deref()),
        };
        type_name.map_or_else(Vec::new, |name| {
            self.lookup(
                candidate.site,
                name.trim_start_matches('?'),
                SymbolKind::Class,
                candidate.owner,
            )
        })
    }

    pub(in crate::resolver::php) fn receiver_binding(
        &self,
        file: usize,
        reference: &Reference,
        name: &str,
    ) -> Option<&crate::graph::BindingType> {
        let semantic = self.graph.semantic(
            file,
            reference.context_id,
            Some(ResolutionFamily::PhpNamespaces),
        )?;
        let mut bindings = semantic.bindings.iter().filter(|binding| {
            binding.name == name
                && binding.scope_start <= reference.start_byte
                && reference.start_byte < binding.scope_end
                && self.callable_owner(file, binding.name_start_byte) == reference.execution_owner
        });
        let binding = bindings
            .clone()
            .min_by_key(|binding| std::cmp::Reverse(binding.scope_start))?;
        bindings
            .all(|other| {
                other.type_name == binding.type_name && other.alias_target == binding.alias_target
            })
            .then_some(binding)
    }

    pub(in crate::resolver::php) fn callable_owner(
        &self,
        file: usize,
        position: usize,
    ) -> Option<usize> {
        self.graph
            .file(file)
            .analysis
            .functions
            .iter()
            .filter(|function| function.start_byte <= position && position < function.end_byte)
            .min_by_key(|function| function.end_byte - function.start_byte)
            .map(|function| function.start_byte)
    }
}
