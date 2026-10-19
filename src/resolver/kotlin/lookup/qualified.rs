use super::*;

impl Index<'_> {
    pub(in crate::resolver::kotlin) fn qualified_candidates(
        &self,
        file: usize,
        reference: &Reference,
        path: &[String],
        package: &[String],
    ) -> Vec<usize> {
        match path {
            [] | [_] => Vec::new(),
            [receiver, name] => self
                .two_segment_receiver_candidates(file, reference, path, package)
                .unwrap_or_else(|| {
                    self.single_segment_package_candidates(file, reference, receiver, name)
                }),
            [receiver, .., name] => {
                if self.graph.has_local_binding(
                    file,
                    receiver,
                    reference.context_id,
                    reference.start_byte,
                ) {
                    Vec::new()
                } else {
                    self.long_qualified_candidates(file, reference, path, name)
                }
            }
        }
    }

    pub(in crate::resolver::kotlin) fn two_segment_receiver_candidates(
        &self,
        file: usize,
        reference: &Reference,
        path: &[String],
        package: &[String],
    ) -> Option<Vec<usize>> {
        let receiver = &path[0];
        let name = path.last()?;
        if is_explicit_receiver(receiver) {
            return Some(
                self.explicit_receiver_candidates(file, reference, receiver, name, package),
            );
        }

        if self
            .graph
            .has_local_binding(file, receiver, reference.context_id, reference.start_byte)
        {
            let ty = self
                .graph
                .binding_type(file, receiver, reference.context_id, reference.start_byte)
                .or_else(|| self.inferred_lambda_parameter_type(file, reference, receiver));
            return Some(ty.map_or_else(Vec::new, |ty| {
                self.typed_receiver_candidates(file, reference, name, &ty, true)
            }));
        }
        if let Some(ty) = self.inferred_lambda_parameter_type(file, reference, receiver) {
            return Some(self.typed_receiver_candidates(file, reference, name, &ty, true));
        }
        if let Some(ty) = self.imported_type(file, reference, receiver) {
            return Some(self.typed_receiver_candidates(file, reference, name, &ty, false));
        }
        if let Some(ty) = self.same_package_type(package, receiver) {
            let allow_extensions = !self.is_static_type_qualifier(file, reference);
            return Some(self.typed_receiver_candidates(
                file,
                reference,
                name,
                &ty,
                allow_extensions,
            ));
        }

        self.namespace_receiver_candidates(file, reference, receiver, name)
    }

    pub(in crate::resolver::kotlin) fn explicit_receiver_candidates(
        &self,
        file: usize,
        reference: &Reference,
        receiver: &str,
        name: &str,
        package: &[String],
    ) -> Vec<usize> {
        let scopes = self.active_receivers(file, reference);
        let scope = receiver
            .strip_prefix("this@")
            .and_then(|label| {
                scopes
                    .iter()
                    .find(|scope| scope.label.as_deref() == Some(label))
            })
            .or_else(|| {
                (!receiver.starts_with("this@"))
                    .then(|| scopes.first())
                    .flatten()
            });
        if let Some(scope) = scope {
            return self.typed_receiver_candidates(
                file,
                reference,
                name,
                &scope.receiver_type,
                true,
            );
        }
        let Some(label) = receiver.strip_prefix("this@") else {
            return Vec::new();
        };
        let Some(class) = self
            .named(label)
            .find(|candidate| candidate.package == package && is_type_declaration(candidate))
        else {
            return Vec::new();
        };
        self.accessible(
            file,
            reference,
            self.members_of_type(file, reference, name, class.name),
        )
    }

    pub(in crate::resolver::kotlin) fn typed_receiver_candidates(
        &self,
        file: usize,
        reference: &Reference,
        name: &str,
        type_name: &str,
        allow_extensions: bool,
    ) -> Vec<usize> {
        let mut candidates = self.members_of_type(file, reference, name, type_name);
        if candidates.is_empty() && allow_extensions {
            candidates = self.extensions_for_type(file, reference, name, type_name);
        }
        self.accessible(file, reference, candidates)
    }

    pub(in crate::resolver::kotlin) fn single_segment_package_candidates(
        &self,
        file: usize,
        reference: &Reference,
        receiver: &str,
        name: &str,
    ) -> Vec<usize> {
        let candidates = self
            .named(name)
            .filter(|candidate| candidate.package == [receiver.to_owned()])
            .filter(|candidate| candidate.owner.is_empty())
            .filter(|candidate| self.visible_in_source_set(file, candidate))
            .map(|candidate| candidate.candidate_index)
            .collect();
        self.accessible(file, reference, candidates)
    }

    pub(in crate::resolver::kotlin) fn long_qualified_candidates(
        &self,
        file: usize,
        reference: &Reference,
        path: &[String],
        name: &str,
    ) -> Vec<usize> {
        if let Some(candidates) =
            self.fully_qualified_package_candidates(file, reference, path, name)
        {
            return candidates;
        }
        if let Some(ty) = self.imported_type(file, reference, &path[0]) {
            return self.typed_receiver_candidates(file, reference, name, &ty, false);
        }
        // Unknown qualified receivers never match a top-level declaration by
        // their final segment.
        self.imported_namespace_path_candidates(file, reference, path)
            .unwrap_or_default()
    }

    pub(in crate::resolver::kotlin) fn fully_qualified_package_candidates(
        &self,
        file: usize,
        reference: &Reference,
        path: &[String],
        name: &str,
    ) -> Option<Vec<usize>> {
        let prefix = &path[..path.len().saturating_sub(1)];
        let candidates = self
            .named(name)
            .filter(|candidate| candidate.package == prefix && candidate.owner.is_empty())
            .filter(|candidate| self.visible_in_source_set(file, candidate))
            .map(|candidate| candidate.candidate_index)
            .collect::<Vec<_>>();
        (!candidates.is_empty()).then(|| self.accessible(file, reference, candidates))
    }
}
