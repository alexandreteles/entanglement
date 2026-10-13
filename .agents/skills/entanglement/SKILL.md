---
name: entanglement
description: Use the installed Entanglement CLI during code changes to compare repository size, function complexity, and maintainability against a baseline. Applies to supported source languages, including Kotlin.
---

# Entanglement

Keep requested code changes maintainable using the installed `entanglement` command. Python 3.14+ runs the bundled comparator. Resolve `skill_dir` to this skill's directory.

Before editing, run from the project root and preserve the actual working state, including existing user changes:

```sh
repo=$(pwd -P)
metrics_dir=$(mktemp -d)
entanglement --version > "$metrics_dir/version.txt"
entanglement repo "$repo" --metrics nloc,cc,cogc,mi --format json > "$metrics_dir/baseline.json"
```

Keep this baseline fixed for the task. Record the root, ignore scope, and existing dirty files; do not reset user changes. Reports contain source hashes. Keep artifacts outside the repository, and use the same version, root, discovery scope, and metrics throughout. Check command success before comparing.

After each meaningful edit batch, analyze the working tree again:

```sh
entanglement repo "$repo" --metrics nloc,cc,cogc,mi --format json > "$metrics_dir/after.json"
python3 "$skill_dir/scripts/compare.py" "$metrics_dir/baseline.json" "$metrics_dir/after.json" \
  --before-root "$repo" --after-root "$repo"
```

The comparator prints compact totals and file changes: exit **0** reports no red edited files, **1** flags red edited files for review, **2** means incomplete analysis. File MI is diagnostic; exit 1 does not require splitting a cohesive module. Both complete outcomes require reviewing growth and function hotspots. Compare full repository NLOC and summed function CC/CogC, affected files, and function hotspots. Reduce avoidable duplication and nesting; justify necessary growth against the requested behavior. Do not impose zero growth: extracting helpers adds baseline CC. Never hide a hotspot behind an average MI, minified lines, or reduced scan coverage.

**Review gate:** examine new or worsened function hotspots alongside total production NLOC, summed CogC, and CC minus function count. Functions above CogC 15 warrant explicit review; preserve or improve their control flow, or explain a necessary exception. Track file count and cohesive module boundaries separately from code size. Justify source growth against the requested behavior; use the same discovery scope and do not hide hotspots by moving or excluding code.

File MI uses the unrounded score: below 10 is red and below 20 is yellow. Investigate low scores, but do not impose a hard file threshold: combining unchanged functions into a cohesive module can lower MI without making those functions more complex. Do not split files or extract trivial helpers solely to improve MI, and do not replace that incentive with a rigid module-size limit.

For competing implementations, generate a root-relative unified diff against preserved **before-state source bytes**, then run:

```sh
entanglement candidate "$before_root" --diff "$proposal_diff" --metrics nloc,cc,cogc,mi --format json
```

Compare its complete `.files` after-state with the matching baseline; `.patch.files` supplies contributor diagnostics but misses untouched callers whose recursion scores change. Use explicit comparator roots when comparing copies. Never apply `git diff` to the already edited tree via `candidate`: it can silently duplicate additions. Snapshot JSON alone does not preserve source bytes. Use `file` or single-file `patch FILE --diff DIFF` for local diagnosis; repository analysis supplies cross-file context. `--diff -` accepts stdin; express renames as deletion/addition (`git diff --no-renames`).

Before finishing, rerun repository analysis and reconcile task files with the inventory, including additions, deletions, untracked and ignored files. Measure missing supported task files explicitly with `file` and include their hotspots and growth in the review; absent metrics are incomplete, not a pass. Run normal syntax/build/tests: Entanglement accepts recovered malformed syntax and exit success does not enforce quality. Distinguish new test failures from pre-existing failures. Report metric deltas, growth rationale, review outcome, and any coverage limitations.
