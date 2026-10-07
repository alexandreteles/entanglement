use std::collections::HashMap;

use tree_sitter::{Node, QueryCapture};

use super::scopes::LocalKind;
use crate::languages::query::QueryAnalyzer;
use crate::model::{DefinitionKind, ReferenceKind};

/// The semantic meaning of one Go query capture.
#[derive(Clone)]
pub(super) enum Role {
    Definition(DefinitionKind),
    Import,
    Local(LocalKind),
    ReceiverTypes,
    Ignored,
    Reference(ReferenceKind),
}

pub(super) struct Captures {
    name: Option<u32>,
    roles: HashMap<u32, Role>,
}

impl Captures {
    pub(super) fn new(query: &QueryAnalyzer) -> Self {
        let roles = [
            (
                "definition.package",
                Role::Definition(DefinitionKind::Module),
            ),
            (
                "definition.function",
                Role::Definition(DefinitionKind::Function),
            ),
            (
                "definition.method",
                Role::Definition(DefinitionKind::Method),
            ),
            ("definition.type", Role::Definition(DefinitionKind::Other)),
            (
                "definition.alias",
                Role::Definition(DefinitionKind::TypeAlias),
            ),
            (
                "definition.constant",
                Role::Definition(DefinitionKind::Constant),
            ),
            (
                "definition.variable",
                Role::Definition(DefinitionKind::Static),
            ),
            ("import", Role::Import),
            ("local.parameter", Role::Local(LocalKind::Parameter)),
            (
                "local.type_parameter",
                Role::Local(LocalKind::TypeParameter),
            ),
            ("local.declaration", Role::Local(LocalKind::Declaration)),
            ("local.switch_alias", Role::Local(LocalKind::SwitchAlias)),
            ("local.receiver_types", Role::ReceiverTypes),
            ("reference.ignored", Role::Ignored),
            ("reference.call", Role::Reference(ReferenceKind::Call)),
            (
                "reference.qualified",
                Role::Reference(ReferenceKind::Qualified),
            ),
            (
                "reference.qualified_type",
                Role::Reference(ReferenceKind::Type),
            ),
            ("reference.type", Role::Reference(ReferenceKind::Type)),
            ("reference.value", Role::Reference(ReferenceKind::Value)),
        ];
        Self {
            name: query.capture_id("definition.name"),
            roles: roles
                .into_iter()
                .filter_map(|(name, role)| query.capture_id(name).map(|id| (id, role)))
                .collect(),
        }
    }

    pub(super) fn role(&self, index: u32) -> Option<Role> {
        self.roles.get(&index).cloned()
    }

    pub(super) fn name<'tree>(&self, captures: &[QueryCapture<'tree>]) -> Option<Node<'tree>> {
        let id = self.name?;
        captures
            .iter()
            .find(|item| item.index == id)
            .map(|item| item.node)
    }
}
