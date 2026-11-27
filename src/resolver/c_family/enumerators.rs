use super::*;

impl Entry<'_> {
    pub(super) fn local_constant(&self) -> bool {
        self.def.kind == DefinitionKind::Constant && self.symbol.internal
    }

    pub(super) fn visible_constant(&self, context: &Context<'_>, at: usize, global: bool) -> bool {
        !self.local_constant()
            || !global
                && self.unit == context.unit
                && self.id.file == context.file
                && (self.def.scope_start..self.def.scope_end).contains(&at)
    }

    /// C++ permits both spellings of an unscoped enum constant.
    pub(super) fn in_scope(&self, scope: &[String]) -> bool {
        self.def.module.0 == scope
            || scope.split_last().is_some_and(|(last, outer)| {
                self.symbol.enum_qualifier.as_ref() == Some(last) && self.def.module.0 == outer
            })
    }
}
