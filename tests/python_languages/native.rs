use super::*;

#[test]
fn python_and_stub_file_selection_and_native_syntax_metrics() {
    let temp = tempfile::tempdir().unwrap();
    for (extension, source, name) in [
        ("py", "def runtime(value): return value\n", "runtime"),
        (
            "pyi",
            "def signature(value: int) -> int: ...\n",
            "signature",
        ),
    ] {
        let path = temp.path().join(format!("module.{extension}"));
        write(&path, source);
        let output = report(&run("file", &path, None, Some("cc,cogc")));
        let file = &output["files"][0];
        assert_eq!(file["language"], "python");
        assert_eq!(function(file, name)["cyclomatic_complexity"], 1);
        assert!(file.get("halstead").is_none());
        assert!(file.get("maintainability_index").is_none());
    }
    let script = temp.path().join("python-script");
    write(
        &script,
        "#!/usr/bin/env python3\ndef shebang_selected():\n    return 1\n",
    );
    let selected = report(&run("file", &script, None, Some("cc")));
    assert_eq!(selected["files"][0]["language"], "python");
    function(&selected["files"][0], "shebang_selected");

    let source = r#"
"""module docstring sentinel"""
# commentOnlySentinel

def decorate(fn): return fn

@decorate
async def flow(flag=False):
    """function docstring sentinel"""
    values = [item for item in range(3) if item and flag]
    try:
        if flag and values:
            return await flow(False)
        elif flag or values:
            return None
    except Exception:
        return None
    return values

def generator(values):
    for value in values:
        yield value

def loop_else(values):
    for value in values:
        pass
    else:
        return 0
    return values

def filtered(rows):
    return [cell for row in rows if row for cell in row]

def outer(flag):
    def inner(value):
        return value if flag else 0
    callback = lambda value: value or 0
    return inner(callback(flag)) + sum(map(lambda value: value + 1, [1]))

def recurse(flag):
    return recurse(not flag)

def classify(value):
    match value:
        case 1:
            return "one"
        case 2 if value > 0:
            return "two"
        case _:
            return "other"

class Worker:
    @decorate
    async def method(self, flag):
        if flag:
            return await flow(False)
        return None
"#;
    let path = temp.path().join("native.py");
    write(&path, source);
    let native = report(&run("file", &path, None, None));
    let native = &native["files"][0];
    for name in [
        "flow",
        "generator",
        "loop_else",
        "filtered",
        "outer",
        "inner",
        "callback",
        "recurse",
        "classify",
        "method",
    ] {
        function(native, name);
    }
    let names: Vec<_> = native["functions"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|item| item["name"].as_str())
        .collect();
    assert!(names.iter().any(|name| name.starts_with("anonymous")));
    assert_eq!(
        names.iter().filter(|name| **name == "flow").count(),
        1,
        "decorators should not duplicate a function"
    );
    assert!(recursive(function(native, "recurse")));
    assert_eq!(function(native, "generator")["cyclomatic_complexity"], 2);
    assert_eq!(function(native, "loop_else")["cyclomatic_complexity"], 2);
    assert_eq!(function(native, "loop_else")["cognitive_complexity"], 2);
    assert_eq!(function(native, "filtered")["cyclomatic_complexity"], 4);
    assert_eq!(function(native, "filtered")["cognitive_complexity"], 6);
    assert_eq!(function(native, "outer")["cyclomatic_complexity"], 1);
    assert!(
        function(native, "inner")["cyclomatic_complexity"]
            .as_u64()
            .unwrap()
            > 1
    );
    assert!(
        function(native, "classify")["cyclomatic_complexity"]
            .as_u64()
            .unwrap()
            >= 3
    );
    assert!(named_operand(
        &native["halstead"],
        "module docstring sentinel"
    ));
    assert!(named_operand(
        &function(native, "flow")["halstead"],
        "function docstring sentinel"
    ));
    assert!(!named_operand(&native["halstead"], "commentOnlySentinel"));
}
