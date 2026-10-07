use super::{report, resolutions, run, status, write};

#[test]
fn ordinary_parameters_do_not_shadow_signature_types() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("scope.go");
    write(
        &path,
        r#"package scope
type T int
func F(T T) T { return T }
func G() (T T) { return }
func External(T T) T
"#,
    );
    let output = report(&run(&["file"], &path, None));
    let refs = resolutions(&output["files"][0]);
    let t = status(&refs, "T");
    assert_eq!(
        t.iter()
            .filter(|state| **state == "exact scope.go:T")
            .count(),
        5
    );
    assert_eq!(t.iter().filter(|state| **state == "unresolved").count(), 1);
}

#[test]
fn generic_type_alias_and_receiver_parameters_shadow_outer_names() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("generics.go");
    write(
        &path,
        r#"package generics
type T int
type Box[T any] struct { Value T }
type Alias[T any] = []T
func (b Box[T]) Get() T { var x T; return x }
func (b *Box[T]) Pointer() T { var x T; return x }
func F[T any](x T) T { return x }
"#,
    );
    let output = report(&run(&["file"], &path, None));
    let refs = resolutions(&output["files"][0]);
    let t = status(&refs, "T");
    assert!(!t.is_empty());
    assert!(t.iter().all(|state| *state == "unresolved"), "{t:?}");
    assert_eq!(status(&refs, "Box"), ["exact generics.go:Box"; 2]);
}

#[test]
fn type_switch_bindings_start_after_each_case_type_list() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("switch.go");
    write(
        &path,
        r#"package switches
type T int
func Inspect(value any) {
    switch T := value.(type) {
    case T: _ = T
    case string: _ = T
    default: _ = T
    }
    var after T
    _ = after
}
"#,
    );
    let output = report(&run(&["file"], &path, None));
    let refs = resolutions(&output["files"][0]);
    let t = status(&refs, "T");
    assert_eq!(
        t.iter()
            .filter(|state| **state == "exact switch.go:T")
            .count(),
        2
    );
    assert_eq!(t.iter().filter(|state| **state == "unresolved").count(), 3);
}

#[test]
fn initializer_and_nested_block_visibility_remain_distinct() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("nested.go");
    write(
        &path,
        r#"package nested
func F() int { return 0 }
func Use() { { F := F(); _ = F }; F() }
"#,
    );
    let output = report(&run(&["file"], &path, None));
    let refs = resolutions(&output["files"][0]);
    assert_eq!(
        status(&refs, "F")
            .iter()
            .filter(|state| **state == "exact nested.go:F")
            .count(),
        2
    );
    assert_eq!(
        status(&refs, "F")
            .iter()
            .filter(|state| **state == "unresolved")
            .count(),
        1
    );
}
