use clap::Parser;

use crate::cli::{Cli, Command};
use crate::metrics::selection::{Metric, Selection};

#[test]
fn cli_accepts_repeated_and_comma_delimited_metrics() {
    let args = Cli::try_parse_from([
        "entanglement",
        "--metrics",
        "cc,density",
        "--metrics",
        "cognitive,maintainability",
        "file",
        "src/main.rs",
    ])
    .unwrap();

    assert_eq!(
        args.metrics,
        [Metric::Cc, Metric::Density, Metric::Cogc, Metric::Mi]
    );
    assert!(matches!(args.command, Command::File { .. }));
}

#[test]
fn cli_defaults_to_all_and_rejects_empty_or_unknown_values() {
    let args = Cli::try_parse_from(["entanglement", "file", "src/main.rs"]).unwrap();
    assert_eq!(args.metrics, [Metric::All]);

    assert!(Cli::try_parse_from(["entanglement", "--metrics", "", "file", "src/main.rs"]).is_err());
    assert!(
        Cli::try_parse_from([
            "entanglement",
            "--metrics",
            "nloc,unknown",
            "file",
            "src/main.rs"
        ])
        .is_err()
    );
}

#[test]
fn selection_tracks_direct_metrics_and_dependencies() {
    let nloc = Selection::new(&[Metric::Nloc]);
    assert!(nloc.includes(Metric::Nloc));
    assert!(!nloc.includes(Metric::Cc));
    assert!(!nloc.needs_cc());
    assert!(!nloc.needs_halstead());

    let derived = Selection::new(&[Metric::Density, Metric::Mi]);
    assert!(derived.needs_cc());
    assert!(derived.needs_halstead());
    assert!(derived.needs_density());
    assert!(derived.needs_mi());
    assert!(!derived.includes(Metric::Cc));
    assert!(!derived.includes(Metric::Halstead));

    let all = Selection::new(&[Metric::All]);
    assert!(all.includes(Metric::All));
    assert!(all.needs_cognitive());
    assert_eq!(Selection::default(), all);
}
