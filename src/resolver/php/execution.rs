//! Connect proven PHP invocations to reported executable ranges.

use super::*;
use crate::model::{ExecutionTarget, ReferenceAccess, SourceRange};

impl Index<'_> {
    pub(super) fn callable_reference(
        &self,
        file: usize,
        reference: &Reference,
        php: &PhpReference,
        seen: &mut HashSet<String>,
    ) -> ReferenceAnalysis {
        if !php.callable || !seen.insert(php.name.clone()) {
            return analysis(reference, Resolution::Unresolved, None);
        }
        let Some(binding) = self
            .receiver_binding(file, reference, &php.name)
            .filter(|binding| binding.immutable)
        else {
            return analysis(reference, Resolution::Unresolved, None);
        };
        let Some(alias) = binding.alias_target.as_deref() else {
            return analysis(reference, Resolution::Unresolved, None);
        };
        if let Some((receiver, name)) = alias
            .strip_prefix("@method:")
            .and_then(|alias| alias.rsplit_once("::"))
        {
            return self.method_callable(file, reference, php, receiver, name);
        }
        if alias.starts_with('$') {
            return self.callable_reference(
                file,
                reference,
                &PhpReference {
                    name: alias.to_owned(),
                    ..php.clone()
                },
                seen,
            );
        }
        let candidates = if let Some(start) = alias
            .strip_prefix("@closure:")
            .and_then(|start| start.parse::<usize>().ok())
        {
            self.candidates
                .iter()
                .enumerate()
                .filter(|(_, candidate)| {
                    candidate.id.file_index == file
                        && candidate.context == reference.context_id
                        && candidate.php.start_byte == start
                })
                .map(|(index, _)| index)
                .collect()
        } else if !alias.starts_with('@') {
            self.lookup(
                file,
                reference.context_id,
                &PhpReference {
                    name: alias.to_owned(),
                    kind: PhpReferenceKind::Function,
                    ..php.clone()
                },
            )
        } else {
            Vec::new()
        };
        self.finish(
            file,
            reference,
            php,
            candidates.into_iter().map(Member::new).collect(),
        )
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
            kind: if receiver.starts_with('$') || receiver.starts_with("new ") {
                PhpReferenceKind::Method
            } else {
                PhpReferenceKind::StaticMethod
            },
            ..php.clone()
        };
        return self.finish(
            file,
            reference,
            &php,
            self.member_reference(file, reference, &php, &mut HashSet::new()),
        );
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
        if php.kind == PhpReferenceKind::Class && php.callable {
            self.constructor(file, reference, php, member.candidate, result);
            return;
        }
        if result.dispatch != Some(CallDispatch::Direct) {
            return;
        }
        let Some(execution) = candidate
            .declaration
            .and_then(|declaration| declaration.execution.as_ref())
        else {
            return;
        };
        let ranges = if candidate.php.kind == PhpSymbolKind::Property {
            match reference.access {
                Some(ReferenceAccess::Read) => execution.getter.into_iter().collect::<Vec<_>>(),
                Some(ReferenceAccess::Write) => execution.setter.into_iter().collect(),
                Some(ReferenceAccess::ReadWrite) => execution
                    .getter
                    .into_iter()
                    .chain(execution.setter)
                    .collect(),
                None => Vec::new(),
            }
        } else if php.callable {
            execution.callable_value.into_iter().collect()
        } else {
            Vec::new()
        };
        for range in ranges {
            // PHP uses the backing property when its own hook accesses $this.
            if php.receiver.as_deref() == Some("$this")
                && reference.execution_owner.is_some_and(|owner| {
                    [execution.getter, execution.setter]
                        .into_iter()
                        .flatten()
                        .any(|hook| hook.start_byte == owner)
                })
            {
                continue;
            }
            append_target(reference, candidate, range, result);
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
        if php.name.eq_ignore_ascii_case("static")
            && !candidate
                .declaration
                .is_some_and(|declaration| declaration.modifiers.is_final)
        {
            result.dispatch = Some(CallDispatch::Virtual);
            return;
        }
        if candidate.php.conditional
            || candidate.php.is_interface
            || candidate.php.is_trait
            || candidate
                .declaration
                .is_some_and(|declaration| declaration.modifiers.is_abstract)
        {
            result.resolution = Resolution::Unresolved;
            result.dispatch = None;
            return;
        }
        let constructors = self.type_members(
            class,
            PhpSymbolKind::Method,
            "__construct",
            &mut HashSet::new(),
        );
        match constructors.as_slice() {
            [] => {}
            [constructor] => {
                let constructor_candidate = &self.candidates[constructor.candidate];
                if !self.accessible(file, reference, php, constructor) {
                    result.resolution = Resolution::Unresolved;
                    result.dispatch = None;
                    return;
                }
                // The constructor symbol identifies inherited executable bodies
                // as well as constructors declared in the allocated class.
                result.resolution = Resolution::Exact(constructor_candidate.symbol.clone());
                result.dispatch = Some(CallDispatch::Direct);
                if let Some(execution) = constructor_candidate
                    .declaration
                    .and_then(|declaration| declaration.execution.as_ref())
                {
                    for range in &execution.constructors {
                        append_target(reference, constructor_candidate, *range, result);
                    }
                }
            }
            _ => {
                result.resolution = Resolution::Ambiguous(
                    constructors
                        .iter()
                        .map(|member| self.candidates[member.candidate].symbol.clone())
                        .collect(),
                );
                result.dispatch = None;
            }
        }
    }
}

fn append_target(
    reference: &Reference,
    candidate: &Candidate<'_>,
    range: SourceRange,
    result: &mut ReferenceAnalysis,
) {
    if let Some(caller_start_byte) = reference.execution_owner.or(reference.call_owner) {
        result.execution_targets.push(ExecutionTarget {
            caller_start_byte,
            file_index: candidate.id.file_index,
            context_id: candidate.context,
            range,
        });
    }
}
