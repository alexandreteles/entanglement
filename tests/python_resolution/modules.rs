use super::*;

#[test]
fn dotted_relative_reexport_and_cyclic_python_imports_resolve_exactly() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("python-project");
    write(&root, "pyproject.toml", "[project]\nname = 'fixture'\n");
    write(
        &root,
        "src/pkg/__init__.py",
        r#"def entry(flag):
    if flag:
        public_leaf(False)

from .mod import leaf as public_leaf
"#,
    );
    write(
        &root,
        "src/pkg/mod.py",
        r#"from . import entry
from .sibling import sibling
from .shared import helper

def leaf(flag):
    if flag:
        entry(False)
    sibling()
    helper()

def _private():
    return 1
"#,
    );
    write(
        &root,
        "src/pkg/sibling.py",
        "def sibling():\n    return 1\n",
    );
    write(&root, "src/pkg/shared.py", "def helper():\n    return 1\n");
    write(&root, "src/pkg/nested/__init__.py", "");
    write(
        &root,
        "src/pkg/nested/worker.py",
        r#"from ..mod import leaf as relative_leaf
from ..shared import helper
from ...outside import distant

def worker(flag):
    if flag:
        relative_leaf(False)
    helper()
    distant()
"#,
    );
    let main_source = r#"from pkg import public_leaf as imported_leaf
import pkg.mod
import pkg.mod as module_alias
from pkg.mod import _private
from requests import get as external_get

def exercise(flag):
    imported_leaf(flag)
    pkg.mod.leaf(flag)
    module_alias.leaf(flag)
    _private()
    external_get()
"#;
    write(&root, "src/main.py", main_source);
    write(
        &root,
        "src/pkg/missing_user.py",
        "from .missing_module import absent\n\ndef use_missing():\n    absent()\n",
    );
    write(&root, "src/pkg/mod.ts", "export function leaf() {}\n");

    let project = analyze(&root);
    let main = file(&project, "/main.py");
    exact_target(main, &["imported_leaf"], "/pkg/mod.py", "leaf");
    exact_target(main, &["pkg", "mod", "leaf"], "/pkg/mod.py", "leaf");
    exact_target(main, &["module_alias", "leaf"], "/pkg/mod.py", "leaf");
    exact_target(main, &["_private"], "/pkg/mod.py", "_private");
    assert_eq!(
        reference(main, &["external_get"])["resolution"]["status"],
        "external"
    );
    assert_eq!(
        reference(file(&project, "/missing_user.py"), &["absent"])["resolution"]["status"],
        "unresolved"
    );

    let package = file(&project, "/__init__.py");
    assert!(recursive(package, "entry"));
    assert!(recursive(file(&project, "/pkg/mod.py"), "leaf"));
    exact_target(
        file(&project, "/worker.py"),
        &["relative_leaf"],
        "/pkg/mod.py",
        "leaf",
    );
    exact_target(
        file(&project, "/worker.py"),
        &["helper"],
        "/pkg/shared.py",
        "helper",
    );
    assert_eq!(
        reference(file(&project, "/worker.py"), &["distant"])["resolution"]["status"],
        "unresolved"
    );
}
