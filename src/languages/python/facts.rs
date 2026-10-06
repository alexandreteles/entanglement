use tree_sitter::Node;

mod bindings;
mod definitions;
mod imports;
mod references;
mod syntax;

use crate::languages::{FileModuleLayout, FileModuleRules};
use crate::model::{Definition, Export, Import, LocalBinding, Reference};

const PYTHON_EXTENSIONS: &[&str] = &["py", "pyi"];
const PYTHON_INDEX_STEMS: &[&str] = &["__init__"];
const PYTHON_ROOTS: &[&str] = &["", "src"];
const PYTHON_PROJECT_MARKERS: &[&str] = &["pyproject.toml", "setup.py", "setup.cfg"];

pub(super) const PYTHON_FILE_MODULE_RULES: FileModuleRules = FileModuleRules {
    extensions: PYTHON_EXTENSIONS,
    remaps: &[],
    index_stems: PYTHON_INDEX_STEMS,
    unresolved_prefixes: &[],
    layout: FileModuleLayout::Dotted {
        roots: PYTHON_ROOTS,
        project_markers: PYTHON_PROJECT_MARKERS,
        submodule_imports: true,
    },
    module_bindings_shadow: true,
    wildcard_imports_shadow: true,
};

#[derive(Default)]
pub(super) struct SemanticFacts {
    pub definitions: Vec<Definition>,
    pub imports: Vec<Import>,
    pub exports: Vec<Export>,
    pub references: Vec<Reference>,
    pub locals: Vec<LocalBinding>,
}

pub(super) fn capture(root: Node<'_>, source: &[u8]) -> SemanticFacts {
    let mut facts = SemanticFacts::default();
    let mut occupied = definitions::collect(root, source, &mut facts);
    occupied.extend(imports::collect(root, source, &mut facts));
    occupied.extend(bindings::collect(root, source, &mut facts));
    facts.references = references::collect(root, source, &occupied);
    facts
}
