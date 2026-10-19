use super::*;

impl Index<'_> {
    pub(in crate::resolver::php) fn legal_member(
        &self,
        file: usize,
        reference: &Reference,
        php: &PhpReference,
        member: &Member,
    ) -> bool {
        let candidate = &self.candidates[member.candidate];
        match php.kind {
            RefKind::StaticProperty => candidate.php.is_static,
            RefKind::Property => !candidate.php.is_static,
            RefKind::StaticMethod if !candidate.php.is_static => {
                php.receiver.as_deref().is_some_and(|receiver| {
                    ["self", "parent", "static"]
                        .iter()
                        .any(|name| receiver.eq_ignore_ascii_case(name))
                }) && self.candidates.iter().any(|caller| {
                    caller.site.file == file
                        && caller.site.context == reference.context_id
                        && Some(caller.site.position) == reference.execution_owner
                        && !caller.php.is_static
                        && caller.php.kind == SymbolKind::Method
                })
            }
            _ => true,
        }
    }

    pub(in crate::resolver::php) fn accessible(
        &self,
        file: usize,
        reference: &Reference,
        php: &PhpReference,
        member: &Member,
    ) -> bool {
        let candidate = &self.candidates[member.candidate];
        let read_visibility = member
            .visibility
            .unwrap_or(candidate.declaration.visibility);
        let visibility = match (candidate.php.kind, reference.access) {
            (SymbolKind::Property, Some(Write | ReadWrite)) => {
                candidate.php.setter_visibility.unwrap_or(read_visibility)
            }
            _ => read_visibility,
        };
        if visibility == Public {
            return true;
        }
        let Some((lexical, owner)) = php.class.as_deref().zip(
            member
                .class
                .map(|class| self.candidates[class].name.as_str())
                .or(candidate.owner),
        ) else {
            return false;
        };
        if lexical.eq_ignore_ascii_case(owner) {
            return true;
        }
        let classes = self.lookup(
            Site::at(file, reference),
            &format!("\\{lexical}"),
            SymbolKind::Class,
            None,
        );
        classes
            .iter()
            .filter(|class| self.candidates[**class].php.is_trait)
            .flat_map(|class| self.consumers(*class, &mut HashSet::new()))
            .any(|class| self.candidates[class].name.eq_ignore_ascii_case(owner))
            || (visibility == Protected
                && classes
                    .into_iter()
                    .any(|class| self.extends(class, owner, &mut HashSet::new())))
    }

    pub(in crate::resolver::php) fn extends(
        &self,
        class: usize,
        owner: &str,
        visited: &mut HashSet<usize>,
    ) -> bool {
        self.candidates[class].name.eq_ignore_ascii_case(owner)
            || (visited.insert(class)
                && self
                    .parents(class)
                    .into_iter()
                    .any(|parent| self.extends(parent, owner, visited)))
    }

    pub(in crate::resolver::php) fn dispatch(
        &self,
        php: &PhpReference,
        member: &Member,
    ) -> CallDispatch {
        let candidate = &self.candidates[member.candidate];
        let declaration = candidate.declaration;
        match candidate.php.kind {
            SymbolKind::Function | SymbolKind::Class => CallDispatch::Direct,
            _ if declaration.modifiers.is_abstract || declaration.is_interface_member => {
                CallDispatch::Virtual
            }
            _ if member.visibility == Some(Private)
                || member.is_final
                || member.exact_receiver
                || declaration.modifiers.is_final
                || declaration.visibility == Private
                || (php.kind == RefKind::StaticMethod
                    && !php
                        .receiver
                        .as_deref()
                        .unwrap_or_default()
                        .eq_ignore_ascii_case("static")) =>
            {
                CallDispatch::Direct
            }
            _ => CallDispatch::Virtual,
        }
    }
}
