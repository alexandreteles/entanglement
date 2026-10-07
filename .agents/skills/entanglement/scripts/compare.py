#!/usr/bin/env python3
r"""Compare Entanglement repository snapshots and gate edited files on MI.

Requires Python 3.14+ and JSON reports selecting nloc, cc, cogc, and mi.
Print a compact JSON summary; exit 0 when no added/modified file is red,
1 when the MI gate fails, or 2 for unreadable/incomplete reports. Untouched
red debt is reported separately. An empty after-state supports deletions;
two empty inventories cannot demonstrate coverage and are rejected.

Example:
    python3 compare.py before.json after.json --before-root /repo \
        --after-root /scratch/repo

Roots identify the directories passed to Entanglement, allowing snapshots
from different copies to compare by relative path. This checks metrics,
not syntax, correctness, ignore-scope consistency, or source ownership.
"""

import argparse
from dataclasses import asdict, dataclass
import json
import math
import os
from pathlib import Path
import sys


@dataclass(frozen=True)
class Metrics:
    """Store one file's source identity and aggregate measured metrics."""

    hash: str
    nloc: int
    cc: int
    cogc: int
    mi: float


def count(value: object) -> int:
    """Return a nonnegative integer metric or raise ValueError."""
    if type(value) is not int or value < 0:
        raise ValueError("NLOC, CC, and CogC must be nonnegative integers")
    return value


def measure(file: dict[str, object]) -> Metrics:
    """Validate required file/function fields and aggregate CC and CogC.

    Classification uses the unrounded numeric MI score. Missing metrics,
    inconsistent ratings, and invalid hashes raise ValueError or KeyError.
    Function MI is intentionally not a gate; the file score is authoritative.
    """
    index = file["maintainability_index"]
    if not isinstance(index, dict):
        raise ValueError("file MI must be an object")
    score = index["score"]
    if type(score) not in (int, float) or not math.isfinite(score) or not 0 <= score <= 100:
        raise ValueError("file MI must be a finite score between 0 and 100")
    rating = "red_low" if score < 10 else "yellow_moderate" if score < 20 else "green_good"
    if index["rating"] != rating:
        raise ValueError("MI rating disagrees with the unrounded score")
    source_hash = file["hash"]
    if not isinstance(source_hash, str) or not source_hash:
        raise ValueError("file hash must be a nonempty string")
    functions = file["functions"]
    if not isinstance(functions, list) or any(not isinstance(function, dict) for function in functions):
        raise ValueError("functions must be a list of objects")
    return Metrics(
        source_hash,
        count(file["nloc"]),
        sum(count(function["cyclomatic_complexity"]) for function in functions),
        sum(count(function["cognitive_complexity"]) for function in functions),
        float(score),
    )


def load_report(path: Path, root: Path) -> dict[str, Metrics]:
    """Load a snapshot keyed by root-relative logical paths.

    Absolute report paths are made relative to root; relative paths already
    identify files below root. Do not resolve symlinks or read source files.
    Duplicate paths and missing required metrics are errors.
    """
    files = json.loads(path.read_text())["files"]
    if not isinstance(files, list):
        raise ValueError("files must be a list")
    result: dict[str, Metrics] = {}
    for file in files:
        if not isinstance(file, dict) or not isinstance(file["path"], str) or not file["path"]:
            raise ValueError("each file must have a nonempty string path")
        source_path = Path(file["path"])
        key = os.path.relpath(source_path, root) if source_path.is_absolute() else os.path.normpath(source_path)
        if key in result:
            raise ValueError(f"duplicate report path: {key}")
        result[key] = measure(file)
    return result


def totals(files: dict[str, Metrics]) -> dict[str, int]:
    """Sum file NLOC and function CC/CogC across the full inventory."""
    return {field: sum(getattr(file, field) for file in files.values()) for field in ("nloc", "cc", "cogc")}


def compare(before: dict[str, Metrics], after: dict[str, Metrics]) -> dict[str, object]:
    """Summarize source edits, contextual metric changes, and the MI gate.

    Hash differences identify edited files; unchanged sources with changed
    metrics are surfaced separately, including cross-file recursion effects.
    Removed files have no invented MI score. Renames appear as removal/addition.
    The caller must keep version, metric selection, and discovery scope equal.
    """
    if not before and not after:
        raise ValueError("no supported files measured in either snapshot")
    edited = {path for path, file in after.items() if path not in before or file.hash != before[path].hash}
    contextual = {path for path in after.keys() & before.keys() if path not in edited and after[path] != before[path]}
    red = sorted(path for path in edited if after[path].mi < 10)
    old_total, new_total = totals(before), totals(after)
    return {
        "gate_passed": not red,
        "files_before": len(before),
        "files_after": len(after),
        "totals_before": old_total,
        "totals_after": new_total,
        "delta": {field: new_total[field] - old_total[field] for field in old_total},
        "added": sorted(after.keys() - before.keys()),
        "removed": sorted(before.keys() - after.keys()),
        "edited": sorted(edited),
        "contextual_changes": sorted(contextual),
        "red_edited": red,
        "untouched_red": sorted(path for path in after.keys() - edited if after[path].mi < 10),
        "changes": [
            {"path": path, "before": asdict(before[path]) if path in before else None,
             "after": asdict(after[path]) if path in after else None}
            for path in sorted(edited | contextual | (before.keys() - after.keys()))
        ],
    }


def main() -> int:
    """Read reports, print the summary, and return the documented exit code."""
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("baseline", type=Path)
    parser.add_argument("after", type=Path)
    parser.add_argument("--before-root", type=Path, required=True)
    parser.add_argument("--after-root", type=Path, required=True)
    args = parser.parse_args()
    try:
        summary = compare(load_report(args.baseline, args.before_root.absolute()),
                          load_report(args.after, args.after_root.absolute()))
    except (OSError, ValueError, KeyError, TypeError, AttributeError) as error:
        print(f"Incomplete metric comparison: {error}", file=sys.stderr)
        return 2
    print(json.dumps(summary, indent=2))
    return 0 if summary["gate_passed"] else 1


if __name__ == "__main__":
    sys.exit(main())
