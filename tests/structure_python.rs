#[path = "../src/structure/effective/python.rs"]
mod python;
use std::collections::BTreeMap;

fn parents(rows: &[(&str, &[&str])]) -> BTreeMap<String, Vec<String>> {
    rows.iter()
        .map(|(unit, bases)| {
            (
                (*unit).into(),
                bases.iter().map(|base| (*base).into()).collect(),
            )
        })
        .collect()
}

#[test]
fn diamond_preserves_python_c3_precedence() {
    let hierarchy = parents(&[("D", &["B", "C"]), ("B", &["A"]), ("C", &["A"])]);
    assert_eq!(
        python::order("D", &hierarchy).unwrap(),
        ["D", "B", "C", "A"]
    );
}

#[test]
fn inconsistent_parent_orders_are_unavailable() {
    let hierarchy = parents(&[("A", &["X", "Y"]), ("B", &["Y", "X"]), ("C", &["A", "B"])]);
    assert_eq!(
        python::order("C", &hierarchy),
        Err("inconsistent_mro".into())
    );
}

#[test]
fn cycles_and_repeated_bases_are_explicit() {
    assert_eq!(
        python::order("A", &parents(&[("A", &["B"]), ("B", &["A"])])),
        Err("inheritance_cycle".into())
    );
    assert_eq!(
        python::order("A", &parents(&[("A", &["A"])])),
        Err("inheritance_cycle".into())
    );
    assert_eq!(
        python::order("A", &parents(&[("A", &["B", "B"])])),
        Err("duplicate_base".into())
    );
}

#[test]
fn deep_inheritance_uses_an_explicit_stack() {
    let hierarchy = (1..512)
        .map(|index| (index.to_string(), vec![(index - 1).to_string()]))
        .collect();
    let order = python::order("511", &hierarchy).unwrap();
    assert_eq!(order.len(), 512);
    assert_eq!(order.last().unwrap(), "0");
}
