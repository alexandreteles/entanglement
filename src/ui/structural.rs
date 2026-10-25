//! Write compact structural summaries. JSON retains complete source evidence.

use std::collections::BTreeMap;
use std::fmt::Write;

use crate::structure::StructuralReport;

/// Append projection, coverage, cycle, and change summaries.
pub(super) fn write(output: &mut String, report: &StructuralReport) {
    let state = &report.state;
    let _ = writeln!(output, "\nStructure: {}", state.snapshot.scope);
    for graph in &state.graphs {
        write_graph(output, graph);
    }
    write_coverage(output, &state.snapshot.coverage);
    let _ = writeln!(
        output,
        "  Effective declarations: {}",
        state.effective.len()
    );
    for declaration in &state.effective {
        write_effective(output, declaration);
    }
    if let Some(delta) = &report.delta {
        write_events(output, delta);
    }
}

fn write_graph(output: &mut String, graph: &crate::structure::model::GraphAnalysis) {
    let pc = graph
        .pc
        .value
        .map_or_else(|| "null".to_owned(), |value| format!("{value:.4}"));
    let _ = writeln!(
        output,
        "  {:?}/{:?}: units {}  edges {}  SCC {}  PC {} ({}/{})",
        graph.granularity,
        graph.projection,
        graph.units.len(),
        graph.edges.len(),
        graph.cycles.len(),
        pc,
        graph.pc.numerator,
        graph.pc.denominator
    );
    for cycle in &graph.cycles {
        let _ = writeln!(output, "    cycle: {}", cycle.members.join(", "));
        let _ = writeln!(output, "      path: {}", cycle.witness.units.join(" -> "));
    }
}

fn write_coverage(output: &mut String, records: &[crate::structure::model::Coverage]) {
    let mut coverage_counts = BTreeMap::new();
    for coverage in records {
        *coverage_counts
            .entry((&coverage.language, coverage.status, coverage.effective))
            .or_insert(0usize) += 1;
    }
    for ((language, status, effective), count) in coverage_counts {
        let _ = writeln!(output, "  {language}: {status:?} {count}  OO {effective:?}");
    }
}

fn write_events(output: &mut String, delta: &crate::structure::model::StructuralDelta) {
    let _ = writeln!(output, "  Structural events: {}", delta.events.len());
    for event in &delta.events {
        let _ = writeln!(
            output,
            "    {} {:?}/{:?}: {}",
            event.kind,
            event.granularity,
            event.projection,
            event.units.join(", ")
        );
        if let Some(path) = event.after_path.as_ref().or(event.before_path.as_ref()) {
            let _ = writeln!(output, "      path: {}", path.units.join(" -> "));
            if let Some(evidence) = path.evidence.first() {
                let _ = writeln!(
                    output,
                    "      source: {}:{}..{}  evidence {}",
                    evidence.path,
                    evidence.start,
                    evidence.end,
                    path.evidence.len()
                );
            }
        }
    }
}

fn write_effective(
    output: &mut String,
    declaration: &crate::structure::effective::EffectiveDeclaration,
) {
    let _ = writeln!(output, "    {} {:?}", declaration.unit, declaration.status);
    for member in &declaration.members {
        let status = if member.required {
            "req"
        } else if member.body.is_some() {
            "body"
        } else {
            "unknown"
        };
        let _ = writeln!(
            output,
            "      {} {} <- {} [{}]",
            member.name, status, member.origin, member.visibility
        );
    }
    if !declaration.obligations.is_empty() {
        let _ = writeln!(
            output,
            "      obligations: {}",
            declaration.obligations.join(", ")
        );
    }
    if !declaration.conflicts.is_empty() {
        let _ = writeln!(
            output,
            "      conflicts: {}",
            declaration.conflicts.join(", ")
        );
    }
}
