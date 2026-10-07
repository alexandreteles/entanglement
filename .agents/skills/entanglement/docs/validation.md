# Workflow validation

Tested on 2026-10-07 with installed Entanglement 0.1.0 and source revision
`dd4c89c` of `alexandreteles/entanglement`. GPT-6-Luna at xhigh ran the
experiments; GPT-6-Astra at medium reviewed the workflow and gate independently.
This record explains the skill's design. Local source code, absolute project
paths, diffs, and raw reports are deliberately not included.

## Coverage

- Scanned all 18 existing project roots with supported files or manifests:
  17 produced nonempty inventories; one produced no supported files. Languages
  observed: Rust, Python, Go, TypeScript, JavaScript, Svelte, HTML, CSS.
- Baseline command: `entanglement repo ROOT --metrics nloc,cc,cogc,mi --format json`.
  Representative runtimes were 0.09–1.31 seconds. The largest report was about
  27.6 MB; selected metrics still include reference and contribution records.
- Repeated the scans after experimentation: every analyzed source BLAKE3 hash
  and file inventory matched, and tracked Git statuses matched initial states.
  Original codebases were read only; materialized variants lived in temporary
  directories. Pre-existing dirty work was preserved.
- Eight virtual candidates across web languages, Rust, and Python matched
  fresh materialized repository reports exactly, including complete `.files`
  metrics and reference records. Candidate timings were about 0.11–0.27 seconds.
- Python, Go, TypeScript, and Svelte changes also matched target-file hashes and
  MI across `patch`, `candidate FILE`, and `candidate ROOT`.
- Exercised human/JSON output, options before/after commands, metric selection,
  repeated flags, aliases, Halstead/density projections, stdin/file diff input,
  and multi-file candidates. Invalid/empty metrics, unsupported source input,
  empty/mismatched/unsafe/binary diffs failed as expected.
- The complete source suite passed: **96 tests**, using an external Cargo target
  directory. Coverage includes additions/deletions/renames, multiple and empty
  hunks, Unicode paths, EOF markers, embedded languages, module resolution,
  recursion, MI boundaries and explanations, and metric projections. TSX was
  tested through these fixtures; no real-project TSX files were discovered.
- A fresh source build and the installed executable produced byte-identical
  selected-metric repository JSON for the Entanglement checkout.

## Findings that changed the skill

| Approach | Observation | Decision |
| --- | --- | --- |
| Fixed repository baseline → edits → fresh repository report | Preserves dirty starting state, includes new files, retains repository resolution context | Default loop |
| Candidate against preserved before-state plus proposed diff | Full after-state exactly matched fresh analysis | Use to compare unapplied alternatives |
| Candidate against already edited source plus forward diff | Some changes failed context matching; additive CSS/JS/Rust changes succeeded and duplicated additions | Prohibit this loop |
| Single-file patch | Imported Go calls were unresolved; mutual recursion was missed | Local diagnostics only |
| Function-only MI check | A Svelte file remained red while all explicit functions were green | Gate aggregate file MI |
| Whole-repository no-red gate | Many unrelated files were already red, including tooling sources | Gate added/modified task files; report untouched debt |
| Metric command success as correctness check | Malformed and invalid-UTF-8 Python still returned success | Require ordinary syntax/build/tests separately |

A behavior-equivalent Python guard-clause refactor reduced function NLOC from
26 to 25 and CogC from 9 to 5, retained CC 5, and increased function MI from
46.89 to 47.31. File MI increased from 38.03 to 38.24. This demonstrates why
the loop reviews several metrics instead of imposing a universal CC or size
cap. Extracting helpers can legitimately increase summed baseline CC.

MI uses the unrounded file score: red below 10, yellow from 10 to below 20,
green from 20. The comparator explicitly enforces the red threshold and
reports complete-inventory metric deltas, including unchanged-hash callers
whose cognitive complexity changes through recursion resolution.

## Independent forward tests

The reviewer found no blocking defects. Ten independent comparator scenarios
passed, including exact MI 10 versus 9.999999, missing MI/CogC, duplicate paths,
unchanged red debt, edited red rejection, deletions, empty inventories, and
contextual complexity changes. Seven automated CLI tests in this repository
cover those gate/inventory invariants and distinct snapshot roots.

Two Luna agents followed the draft skill on disposable project copies:

- Go: added a missing-URL-scheme guard and regression test. Inventory grew from
  9 to 10 files; deltas were +15 NLOC, +4 CC, +3 CogC. The changed source and new
  test scored 24.79 and 58.06, both green. Unrelated red debt remained visible.
  All packages passed `go test -mod=readonly ./...`; final analysis reproduced
  the comparison summary.
- TypeScript: added an unauthenticated health endpoint and test. Deltas were
  +12 NLOC, +3 CC, unchanged CogC. Edited files scored 41.20 and 44.50, both
  green. Targeted tests passed 3/3. The full suite had 10 passes and 2 failures
  before the change, then 11 passes and the same 2 failures afterward. These
  existing tests supplied an obsolete flat payload to a nested schema; the
  skill now explicitly distinguishes new failures from baseline failures.

The skill-creator validator passed. Both new Python files also satisfy the
skill's no-red file gate. Neither forward test modified its original project.

## Reproducing checks

Requires Python 3.14+, Entanglement on PATH, and PyYAML for the skill-creator
validator. The skill itself and its comparator use no third-party Python
packages. From this repository:

```sh
python3 -m unittest discover -s tests -v
entanglement repo . --metrics nloc,cc,cogc,mi --format json
python3 /path/to/skill-creator/scripts/quick_validate.py .
```

For Entanglement source verification, run `cargo test --locked --target-dir
/temporary/target` in its checkout. Preserve original sources when comparing
candidate output to materialized after-state reports. Raw JSON comparison
requires equivalent logical roots; the bundled comparator supports distinct
roots for metric comparisons.
