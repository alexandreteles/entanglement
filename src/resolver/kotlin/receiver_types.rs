use super::*;

impl Index<'_> {
    pub(super) fn is_super_reference(&self, file: usize, reference: &Reference) -> bool {
        let _ = file;
        reference.path.first().is_some_and(|part| part == "super")
    }
    pub(super) fn method_receiver_type(
        &self,
        file: usize,
        reference: &Reference,
        is_super: bool,
    ) -> Option<String> {
        let containing_call = self.containing_call_facts(file, reference);
        if containing_call
            .is_some_and(|call| call.convention.is_some() && call.receiver_type.is_none())
        {
            return None;
        }
        if let Some(ty) = containing_call.and_then(|call| call.receiver_type.clone()) {
            return Some(ty);
        }
        if is_super {
            return self.super_receiver_type(file, reference);
        }
        if let Some(receiver) = reference.path.first()
            && (receiver == "this" || receiver.starts_with("this@"))
        {
            return self.this_receiver_type(file, reference, receiver);
        }
        let receiver = reference.path.first()?;
        if self
            .graph
            .has_local_binding(file, receiver, reference.context_id, reference.start_byte)
            || receiver == "it"
        {
            return self
                .graph
                .binding_type(file, receiver, reference.context_id, reference.start_byte)
                .or_else(|| self.inferred_lambda_parameter_type(file, reference, receiver));
        }
        let package = self.semantic_package(file, reference.context_id);
        self.imported_type(file, reference, receiver)
            .or_else(|| self.same_package_type(package, receiver))
    }
    pub(super) fn containing_call_facts(
        &self,
        file: usize,
        reference: &Reference,
    ) -> Option<&crate::graph::CallFacts> {
        self.graph
            .call_facts(
                file,
                reference.context_id,
                Some(ResolutionFamily::KotlinModules),
            )
            .iter()
            .filter(|call| {
                call.start_byte <= reference.start_byte && reference.end_byte <= call.end_byte
            })
            .min_by_key(|call| (call.end_byte - call.start_byte, call.convention.is_none()))
    }
    pub(super) fn semantic_package(&self, file: usize, context_id: usize) -> &[String] {
        self.graph
            .semantic(file, context_id, Some(ResolutionFamily::KotlinModules))
            .and_then(|semantic| semantic.context.as_ref())
            .map_or(&[], |context| context.package.as_slice())
    }
    pub(super) fn super_receiver_type(&self, file: usize, reference: &Reference) -> Option<String> {
        let class = self
            .candidates
            .iter()
            .filter(|candidate| candidate.file == file)
            .filter(|candidate| {
                candidate
                    .declaration
                    .as_ref()
                    .is_some_and(|item| item.kind == crate::graph::DeclarationKind::Class)
            })
            .filter(|candidate| {
                candidate.start <= reference.start_byte && reference.start_byte < candidate.end
            })
            .min_by_key(|candidate| candidate.end - candidate.start)?;
        let declaration = class.declaration.as_ref()?;
        let parents = declaration
            .supertypes
            .iter()
            .flat_map(|supertype| {
                self.resolve_declared_type(class.file, class.context_id, supertype, &class.package)
            })
            .collect::<Vec<_>>();
        match parents.as_slice() {
            [parent] => Some(candidate_type_name(parent)),
            _ => None,
        }
    }
    pub(super) fn this_receiver_type(
        &self,
        file: usize,
        reference: &Reference,
        receiver: &str,
    ) -> Option<String> {
        if let Some(label) = receiver.strip_prefix("this@") {
            if let Some(scope) = self
                .active_receivers(file, reference)
                .into_iter()
                .find(|scope| scope.label.as_deref() == Some(label))
            {
                return Some(scope.receiver_type);
            }
            let types = self
                .named(label)
                .filter(|candidate| {
                    candidate.file == file
                        && candidate.package == self.semantic_package(file, reference.context_id)
                        && is_type_declaration(candidate)
                })
                .map(candidate_type_name)
                .collect::<BTreeSet<_>>();
            return (types.len() == 1)
                .then(|| types.into_iter().next())
                .flatten();
        }
        self.candidates
            .iter()
            .filter(|candidate| candidate.file == file)
            .filter(|candidate| {
                candidate.declaration.as_ref().is_some_and(|item| {
                    matches!(
                        item.kind,
                        crate::graph::DeclarationKind::Class
                            | crate::graph::DeclarationKind::Object
                            | crate::graph::DeclarationKind::CompanionObject
                    )
                })
            })
            .filter(|candidate| {
                candidate.start <= reference.start_byte && reference.start_byte < candidate.end
            })
            .min_by_key(|candidate| candidate.end - candidate.start)
            .map(candidate_type_name)
    }
}
