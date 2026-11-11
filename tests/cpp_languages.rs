mod support;
use support::{file, function, recursive, report, resolution_statuses, run, write};

const SHAPES: &str = r#"#include <vector>
namespace geometry {
namespace {
int clamp(int value) { return value < 0 ? 0 : value; }
}
class Shape {
public:
  int area() const { return scale(width); }
  int scale(int value) const;
private:
  int width = 2;
};
int Shape::scale(int value) const { return clamp(value) * 2; }
int total(int count) { return count > 0 ? total(count - 1) : geometry::clamp(count); }
}
int run(const std::vector<int> &items) {
  auto twice = [](int value) { return value * 2; };
  int sum = 0;
  for (const auto &item : items) {
    try { sum += twice(geometry::total(item)); } catch (...) { return 0; }
  }
  return sum;
}
"#;

fn shapes() -> serde_json::Value {
    let temp = tempfile::tempdir().unwrap();
    write(&temp.path().join("shape.cpp"), SHAPES);
    write(
        &temp.path().join("other.cpp"),
        "namespace geometry { int caller() { return clamp(1); } }\n",
    );
    report(&run("repo", temp.path(), None, None))
}

#[test]
fn cpp_metrics_cover_methods_lambdas_and_handlers() {
    let output = shapes();
    let shape = file(&output, "shape.cpp");
    assert_eq!(shape["language"], "cpp");
    assert_eq!(function(shape, "area")["cyclomatic_complexity"], 1);
    assert_eq!(function(shape, "Shape::scale")["cyclomatic_complexity"], 1);
    assert_eq!(function(shape, "twice")["cyclomatic_complexity"], 1);
    let run = function(shape, "run");
    assert_eq!(run["cyclomatic_complexity"], 3);
    assert_eq!(run["cognitive_complexity"], 3);
    assert!(recursive(function(shape, "total")));
}

#[test]
fn cpp_references_follow_namespaces_and_classes() {
    let output = shapes();
    let shape = file(&output, "shape.cpp");
    assert_eq!(
        resolution_statuses(shape, "scale"),
        ["exact shape.cpp:scale"]
    );
    assert_eq!(
        resolution_statuses(shape, "width"),
        ["exact shape.cpp:width"]
    );
    assert_eq!(
        resolution_statuses(shape, "clamp"),
        ["exact shape.cpp:clamp"]
    );
    assert_eq!(
        resolution_statuses(shape, "geometry::clamp"),
        ["exact shape.cpp:clamp"]
    );
    assert_eq!(
        resolution_statuses(shape, "geometry::total"),
        ["exact shape.cpp:total"]
    );
    assert_eq!(resolution_statuses(shape, "twice"), ["unresolved"]);
    assert_eq!(resolution_statuses(shape, "std::vector"), ["external"]);
    let other = file(&output, "other.cpp");
    assert_eq!(resolution_statuses(other, "clamp"), ["unresolved"]);
}

#[test]
fn cpp_constructor_initializers_name_members_and_bases() {
    let temp = tempfile::tempdir().unwrap();
    write(
        &temp.path().join("scope.cpp"),
        "struct Base { int id; };\nstruct Scope : Base {\n  Scope() : Base(), size_(0) {}\n  Scope(int size);\n  int size_;\n};\nScope::Scope(int size) : Base(), size_(size) {}\n",
    );
    let output = report(&run("repo", temp.path(), None, None));
    let scope = file(&output, "scope.cpp");
    assert_eq!(
        resolution_statuses(scope, "size_"),
        ["exact scope.cpp:size_"; 2]
    );
    assert_eq!(
        resolution_statuses(scope, "Base"),
        ["exact scope.cpp:Base"; 3]
    );
}

#[test]
fn cpp_constructor_parameters_bind_in_initializer_lists() {
    let temp = tempfile::tempdir().unwrap();
    write(
        &temp.path().join("scope.cpp"),
        "int size = 4;\nint other = 5;\nstruct Scope {\n  Scope(int size) : size_(size), other_(other) {}\n  Scope(int size, int extra);\n  int size_;\n  int other_;\n};\nScope::Scope(int size, int extra) : size_(size), other_(other) {}\n",
    );
    let output = report(&run("repo", temp.path(), None, None));
    let scope = file(&output, "scope.cpp");
    assert_eq!(resolution_statuses(scope, "size"), ["unresolved"; 2]);
    assert_eq!(
        resolution_statuses(scope, "other"),
        ["exact scope.cpp:other"; 2]
    );
}
