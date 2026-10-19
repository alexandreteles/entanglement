//! Connect proven PHP invocations to reported executable ranges.

use super::*;
use crate::model::{
    ExecutionTarget,
    ReferenceAccess::{Read, ReadWrite, Write},
};
use PhpReferenceKind as RefKind;
use PhpSymbolKind as SymbolKind;

impl Index<'_> {
    pub(super) fn callable_reference(
        &self,
        file: usize,
        reference: &Reference,
        php: &PhpReference,
    ) -> ReferenceAnalysis {
        let alias = match self.callable_alias(file, reference, &php.name) {
            Some(alias) if php.callable => alias,
            _ => return analysis(reference, Resolution::Unresolved, None),
        };
        let candidates = match alias {
            alias if alias.starts_with("@method:") => {
                return match alias[8..].rsplit_once("::") {
                    Some((receiver, name)) => {
                        self.method_callable(file, reference, php, receiver, name)
                    }
                    None => self.finish(file, reference, php, Vec::new()),
                };
            }
            alias if alias.starts_with("@closure:") => {
                let start = alias[9..].parse::<usize>().ok();
                self.candidates
                    .iter()
                    .enumerate()
                    .filter(|(_, candidate)| {
                        candidate.site.file == file
                            && candidate.site.context == reference.context_id
                            && Some(candidate.site.position) == start
                    })
                    .map(|(index, _)| index)
                    .collect()
            }
            alias if !alias.starts_with('@') => self.lookup(
                Site::at(file, reference),
                alias,
                SymbolKind::Function,
                php.class.as_deref(),
            ),
            _ => Vec::new(),
        };
        self.finish(
            file,
            reference,
            php,
            candidates.into_iter().map(Member::new).collect(),
        )
    }

    fn callable_alias<'a>(
        &'a self,
        file: usize,
        reference: &Reference,
        mut name: &'a str,
    ) -> Option<&'a str> {
        let mut seen = HashSet::new();
        loop {
            if !seen.insert(name) {
                return None;
            }
            name = self
                .receiver_binding(file, reference, name)
                .filter(|binding| binding.immutable)?
                .alias_target
                .as_deref()?;
            if !name.starts_with('$') {
                return Some(name);
            }
        }
    }

    fn method_callable(
        &self,
        file: usize,
        reference: &Reference,
        php: &PhpReference,
        receiver: &str,
        name: &str,
    ) -> ReferenceAnalysis {
        if receiver.starts_with('$')
            && receiver != "$this"
            && !self
                .receiver_binding(file, reference, receiver)
                .is_some_and(|binding| binding.immutable)
        {
            return analysis(reference, Resolution::Unresolved, None);
        }
        let php = PhpReference {
            name: name.to_owned(),
            receiver: Some(receiver.to_owned()),
            kind: match receiver.starts_with('$') || receiver.starts_with("new ") {
                true => RefKind::Method,
                false => RefKind::StaticMethod,
            },
            ..php.clone()
        };
        self.finish(
            file,
            reference,
            &php,
            self.member_reference(file, reference, &php, &mut HashSet::new()),
        )
    }

    pub(super) fn execution_targets(
        &self,
        file: usize,
        reference: &Reference,
        php: &PhpReference,
        member: &Member,
        result: &mut ReferenceAnalysis,
    ) {
        let candidate = &self.candidates[member.candidate];
        if php.kind == RefKind::Class && php.callable {
            self.constructor(file, reference, php, member.candidate, result);
            return;
        }
        let execution = match (
            result.dispatch,
            candidate.php.kind,
            candidate.declaration.execution.as_ref(),
        ) {
            (Some(CallDispatch::Direct), SymbolKind::Property, Some(execution)) => execution,
            _ => return,
        };
        // An access from either hook uses this property's backing storage.
        if php.receiver.as_deref() == Some("$this")
            && reference.execution_owner.is_some_and(|owner| {
                [execution.getter, execution.setter]
                    .into_iter()
                    .flatten()
                    .any(|hook| hook.start_byte == owner)
            })
        {
            return;
        }
        let ranges = match reference.access {
            Some(Read) => [execution.getter, None],
            Some(Write) => [execution.setter, None],
            Some(ReadWrite) => [execution.getter, execution.setter],
            _ => [None, None],
        };
        if let Some(caller_start_byte) = reference.execution_owner.or(reference.call_owner) {
            result
                .execution_targets
                .extend(ranges.into_iter().flatten().map(|range| ExecutionTarget {
                    caller_start_byte,
                    file_index: candidate.site.file,
                    context_id: candidate.site.context,
                    range,
                }));
        }
    }

    fn constructor(
        &self,
        file: usize,
        reference: &Reference,
        php: &PhpReference,
        class: usize,
        result: &mut ReferenceAnalysis,
    ) {
        let candidate = &self.candidates[class];
        if php.name.eq_ignore_ascii_case("static") && !candidate.declaration.modifiers.is_final {
            result.dispatch = Some(CallDispatch::Virtual);
            return;
        }
        let constructors = self.type_members(
            class,
            SymbolKind::Method,
            "__construct",
            &mut HashSet::new(),
        );
        let resolution = match constructors.as_slice() {
            _ if candidate.php.conditional
                || candidate.php.is_interface
                || candidate.php.is_trait
                || candidate.declaration.modifiers.is_abstract =>
            {
                Resolution::Unresolved
            }
            [] => return,
            [constructor] if self.accessible(file, reference, php, constructor) => {
                let target = &self.candidates[constructor.candidate];
                result.resolution = Resolution::Exact(target.symbol.clone());
                result.dispatch = Some(CallDispatch::Direct);
                return;
            }
            [_] => Resolution::Unresolved,
            _ => Resolution::Ambiguous(
                constructors
                    .iter()
                    .map(|member| self.candidates[member.candidate].symbol.clone())
                    .collect(),
            ),
        };
        result.resolution = resolution;
        result.dispatch = None;
    }
}
