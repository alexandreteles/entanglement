#!/usr/bin/env python3
"""Compare Entanglement's source reports without mixing before/after path roots."""

import argparse
import collections
import json
from pathlib import Path


def load(path, root):
    report = json.loads(Path(path).read_text(encoding="utf-8"))
    return {Path(f["path"]).relative_to(root).as_posix(): f for f in report["files"]}


def functions(files):
    result = {}
    for path, file in files.items():
        seen = collections.Counter()
        for function in file["functions"]:
            name = function["name"]
            result[path, name, seen[name]] = function
            seen[name] += 1
    return result


def totals(files):
    scopes = list(functions(files).values())
    return {
        "files": len(files),
        "nloc": sum(f["nloc"] for f in files.values()),
        "functions": len(scopes),
        "cyclomatic_complexity": sum(f["cyclomatic_complexity"] for f in scopes),
        "cognitive_complexity": sum(f["cognitive_complexity"] for f in scopes),
    }


def changed_files(before, after):
    result = []
    for path in sorted(before.keys() | after.keys()):
        old, new = before.get(path), after.get(path)
        if old and new and old["hash"] == new["hash"]:
            continue
        result.append({
            "path": path,
            "nloc_before": old["nloc"] if old else 0,
            "nloc_after": new["nloc"] if new else 0,
            "mi_before": old["maintainability_index"]["score"] if old else None,
            "mi_after": new["maintainability_index"]["score"] if new else None,
        })
    return result


def summarize(before, after):
    old, new = functions(before), functions(after)
    added = {key: value for key, value in new.items() if key not in old}
    increases = []
    for key in sorted(old.keys() & new.keys()):
        cc = new[key]["cyclomatic_complexity"] - old[key]["cyclomatic_complexity"]
        cogc = new[key]["cognitive_complexity"] - old[key]["cognitive_complexity"]
        if cc > 0 or cogc > 0:
            increases.append({"path": key[0], "function": key[1], "cc_delta": cc, "cogc_delta": cogc})
    left, right = totals(before), totals(after)
    return {
        "scope": "src/ only; excludes tests, queries, grammar metadata, and CI",
        "before": left, "after": right,
        "delta": {key: right[key] - left[key] for key in left},
        "new_functions": len(added),
        "new_function_max_cc": max((f["cyclomatic_complexity"] for f in added.values()), default=0),
        "new_function_max_cogc": max((f["cognitive_complexity"] for f in added.values()), default=0),
        "existing_function_complexity_increases": increases,
        "files": changed_files(before, after),
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("before")
    parser.add_argument("after")
    parser.add_argument("--before-root", type=Path, required=True)
    parser.add_argument("--after-root", type=Path, required=True)
    args = parser.parse_args()
    before = load(args.before, args.before_root.resolve())
    after = load(args.after, args.after_root.resolve())
    print(json.dumps(summarize(before, after), indent=2))


if __name__ == "__main__":
    main()
