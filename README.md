# Entanglement

Entanglement uses Tree-sitter to measure source code.

Build the program with the stable Rust toolchain:

```sh
cargo build --release
```

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
It skips `.git` and does not follow symbolic links. The `candidate` command
also includes old files named in the diff, even if Git ignore rules exclude them.

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
Diff paths must stay below the selected root directory. They must not
contain symbolic links. The program does not accept binary patches.

| Metric | Counting rule in Entanglement | Paper or publication |
| --- | --- | --- |
| NLOC (non-comment lines of code) | Count each source line covered by a code token once. Exclude comments and syntax that belongs to an unsupported embedded language. | Robert E. Park, [*Software Size Measurement: A Framework for Counting Source Statements*](https://www.sei.cmu.edu/library/software-size-measurement-a-framework-for-counting-source-statements/), CMU/SEI-92-TR-020 (1992). Defines a framework for physical source-line counting rules. Entanglement uses the rule in this row. |
| Cyclomatic complexity (CC) | Start each function at 1. Add 1 for each control-flow decision and logical condition. For a `match` expression with N arms, add `max(N - 1, 0)`. | [NIST SP 500-235, *Structured Testing: A Testing Methodology Using the Cyclomatic Complexity Metric*](https://nvlpubs.nist.gov/nistpubs/Legacy/SP/nistspecialpublication500-235.pdf) (1996), sections 2.2 and 4.1. Defines the metric and decision-counting method. |

Each complexity contribution includes its kind, byte range, and source line.
Complexity density is `CC / max(NLOC, 1)`. A patch report gives the complete
scores before and after the patch.

Use `--format human` for a terminal report. This is the default format.
Use `--format json` for a JSON document. You can put `--format` before or
after the command.

JSON includes file metrics, function contributions, and ranges for embedded
languages. It also includes code reference results and patch comparisons
when applicable.

Code reference results have one of four states: exact, ambiguous, external,
or unresolved. References that need type inference or trait dispatch stay
unresolved. If several crate roots declare the same source file, references
from that file to other files stay unresolved. The resolver does not select
one crate.
