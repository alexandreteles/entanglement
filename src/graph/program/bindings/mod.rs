mod aliases;

use super::{ProgramBinding, ProgramGraph};

impl<'a> ProgramGraph<'a> {
    pub(crate) fn bindings_named(
        &self,
        file_index: usize,
        name: &str,
        context_id: usize,
        position: usize,
    ) -> impl Iterator<Item = &ProgramBinding> {
        self.bindings_by_file_name[file_index]
            .get(name)
            .into_iter()
            .flatten()
            .map(|index| &self.bindings[*index])
            .filter(move |binding| {
                binding.context_id == context_id
                    && binding.start_byte <= position
                    && binding.scope.start_byte <= position
                    && position < binding.scope.end_byte
            })
    }

    /// Return the unique nearest binding for a name at a source position.
    /// A narrower lexical scope wins; declarations in that same scope use
    /// the latest declaration before the position. Equal-ranked alternatives
    /// are ambiguous and do not provide evidence.
    pub(super) fn binding_at(
        &self,
        file_index: usize,
        name: &str,
        context_id: usize,
        position: usize,
    ) -> Option<&ProgramBinding> {
        let bindings = self
            .bindings_named(file_index, name, context_id, position)
            .collect::<Vec<_>>();
        let narrowest = bindings
            .iter()
            .map(|binding| {
                binding
                    .scope
                    .end_byte
                    .saturating_sub(binding.scope.start_byte)
            })
            .min()?;
        let nearest = bindings
            .into_iter()
            .filter(|binding| {
                binding
                    .scope
                    .end_byte
                    .saturating_sub(binding.scope.start_byte)
                    == narrowest
            })
            .collect::<Vec<_>>();
        let scope = nearest.first()?.scope;
        if nearest.iter().any(|binding| {
            binding.scope.start_byte != scope.start_byte || binding.scope.end_byte != scope.end_byte
        }) {
            return None;
        }
        let latest = nearest.iter().map(|binding| binding.start_byte).max()?;
        let mut winners = nearest
            .into_iter()
            .filter(|binding| binding.start_byte == latest);
        let winner = winners.next()?;
        (winners.next().is_none() && !winner.conflicting_evidence).then_some(winner)
    }

    pub(crate) fn has_local_binding(
        &self,
        file_index: usize,
        name: &str,
        context_id: usize,
        position: usize,
    ) -> bool {
        self.bindings_named(file_index, name, context_id, position)
            .next()
            .is_some()
    }
}

pub(super) fn binding_identity(
    binding: &ProgramBinding,
) -> (usize, usize, &str, usize, usize, usize, usize) {
    (
        binding.file_index,
        binding.context_id,
        &binding.name,
        binding.start_byte,
        binding.end_byte,
        binding.scope.start_byte,
        binding.scope.end_byte,
    )
}
