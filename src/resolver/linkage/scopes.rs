//! Order the namespaces and classes that C++ name lookup searches.

use crate::graph::FileFactsView;
use crate::model::Reference;

/// Return groups of candidate scopes for the reference's final name. Lookup
/// stops at the first group with a match. A using-declaration names its
/// target directly; enclosing scopes follow from the innermost outward, and
/// using-directives join the global group.
pub(super) fn candidates(fact: &FileFactsView<'_>, reference: &Reference) -> Vec<Vec<Vec<String>>> {
    let (name, qualifier) = reference.path.split_last().expect("references have a name");
    let module = &reference.module.0;
    let usings = fact.imports().filter(|import| {
        import.source.is_none()
            && import.scope_start <= reference.start_byte
            && reference.start_byte <= import.scope_end
    });
    let mut declared = Vec::new();
    let mut directives = Vec::new();
    for using in usings {
        if using.namespace {
            directives.push(using.path.clone());
        } else if qualifier.is_empty() && using.imported_name.as_ref() == Some(name) {
            declared.push(using.path[..using.path.len() - 1].to_vec());
        }
    }
    let mut groups = vec![declared];
    for depth in (0..=module.len()).rev() {
        groups.push(vec![module[..depth].to_vec()]);
    }
    groups.last_mut().expect("global group").extend(directives);
    for group in &mut groups {
        for scope in group.iter_mut() {
            scope.extend(qualifier.iter().cloned());
        }
    }
    groups
}
