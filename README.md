# Entanglement

Entanglement uses Tree-sitter to measure source code.

Build the program with the stable Rust toolchain:

```sh
cargo build --release
```

Run tests with `cargo test --locked`. Keep tests under `tests/`, covering
distinct observable behaviors without duplicate cases. Do not add test modules
or test-only hooks to `src/`.

Use one of these commands:

```sh
entanglement file src/main.rs
entanglement repo src --format json
entanglement patch src/main.rs --diff change.patch
entanglement candidate . --diff change.patch --format json
entanglement candidate src/main.rs --diff change.patch
git diff -- src/main.rs | entanglement candidate . --diff -
```

| Command | Input | Operation |
| --- | --- | --- |
| `file` | One source file | Analyze the complete file. |
| `repo` | One directory | Find supported source files below the directory. Use Git ignore rules. Analyze the files in parallel. |
| `patch` | One source file and `--diff` | Apply one file patch in memory. Compare the complete file before and after the patch. |
| `candidate` | One directory or source file and `--diff` | Apply the diff in memory. Analyze the source files before and after the change. Use the surrounding repository for a file input. |

The file search includes hidden files unless Git ignore rules exclude them.
It skips `.git` and follows symbolic links to files and directories. It
analyzes each file on disk once and keeps all discovered paths for module
resolution. The `candidate` command also includes old files named in the diff,
even if Git ignore rules exclude them. A broken link or a directory link cycle
causes an error.

For a `candidate` file input, use the nearest directory with `Cargo.toml` or
`.git` to resolve code references. If there is no such directory, use the root
directory from the diff path.

Supply a unified diff with `--diff PATH`. Use `--diff -` to read the diff from
standard input. For a directory input, diff paths are relative to that
directory. For a file input, the diff must contain one file patch. Its old
path must match the selected file. The `patch` and `candidate` commands do
not write source files.

A unified diff can contain several change blocks, called hunks. For a
directory input, it can change several files. It can create, delete, or
rename files. The unchanged lines in each hunk must match the source.
Diff paths must stay below the selected root as written. They can pass through
symbolic links, including links to targets outside that root. The program does
not accept binary patches. A patch with the same old and new paths keeps the
file's report path and aliases. A deletion removes the file and all its
aliases. A rename removes the old file and its aliases, then adds the new path.
A create or rename fails if its destination already exists. This includes a
symbolic link.

| Metric | Counting rule in Entanglement | Paper or publication |
| --- | --- | --- |
| NLOC (non-comment lines of code) | Count each source line covered by a code token once. Exclude comments and syntax that belongs to an unsupported embedded language. | Robert E. Park, [*Software Size Measurement: A Framework for Counting Source Statements*](https://www.sei.cmu.edu/library/software-size-measurement-a-framework-for-counting-source-statements/), CMU/SEI-92-TR-020 (1992). Defines a framework for physical source-line counting rules. Entanglement uses the rule in this row. |
| Cyclomatic complexity (CC) | Start each function at 1. Add 1 for each control-flow decision and logical condition. For a `match` expression with N arms, add `max(N - 1, 0)`. | [NIST SP 500-235, *Structured Testing: A Testing Methodology Using the Cyclomatic Complexity Metric*](https://nvlpubs.nist.gov/nistpubs/Legacy/SP/nistspecialpublication500-235.pdf) (1996), sections 2.2 and 4.1. Defines the metric and decision-counting method. |
| Cognitive complexity (CogC) | Start each function at 0. Each `if`, loop, `let ... else`, or `match` adds 1 plus the current nesting depth. `else` adds 1; an `else if` adds one branch point without treating it as nested under the preceding `if`. A `match` has no additional per-arm points, and a guard adds a nested condition. Closures and nested functions add nesting for their contents; async blocks do not add nesting. Add 1 for the first `&&` or `\|\|` operator in a logical sequence, then for each change between operator kinds; parentheses keep a sequence together and negation splits it without adding a point. Add 1 for a labeled `break` or `continue`. A recursive function gets 1 point when it belongs to a detected direct or mutual call cycle; ordinary calls are free. | SonarSource, [*Cognitive Complexity*](https://www.sonarsource.com/docs/CognitiveComplexity.pdf). Entanglement maps its flow-break, nesting, and logical-sequence principles to captured Rust syntax. |

Each CC and CogC contribution includes its kind, byte range, and source line.
CC density is `CC / max(NLOC, 1)`. A patch report gives the complete scores
before and after the patch, and lists CC and CogC contribution changes
separately.

CogC is derived from Rust syntax captures and does not claim complete semantic
coverage or full parity with Sonar's Rust analyzer. These rules describe
Entanglement's Rust mappings of the paper's counting principles. Recursive
points use Entanglement's exact reference resolutions and
a strongly connected component pass: each function gets one point when a
direct call edge places it in a self-recursive or mutually recursive cycle.
Ambiguous or unresolved targets, methods, calls through function values, and
calls inside closures or async blocks do not form graph edges. The analyzer
does not infer trait or dynamic dispatch, function-pointer calls, or
macro-generated calls. It only sees files supplied to the current analysis,
so cycles through omitted files or injected fragments can be missed. `?`,
ordinary returns and jumps, and logical negation do not add CogC points.

Use `--format human` for a terminal report. This is the default format.
Use `--format json` for a JSON document. You can put `--format` before or
after the command.

JSON includes file metrics, CC and CogC function contributions, and ranges
for embedded languages. Patch comparisons include before/after CogC values
and separate CC and CogC contribution changes. JSON also includes code
reference results when applicable.

Code reference results have one of four states: exact, ambiguous, external,
or unresolved. References that need type inference or trait dispatch stay
unresolved. The same file on disk can appear in several modules in one crate.
The resolver combines reference results from these module contexts. If the
contexts resolve a name to different symbols, the result is ambiguous or
unresolved. If several crate roots declare the same source file, references
from that file to other files stay unresolved. The resolver does not select
one crate.
