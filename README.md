# Entanglement

Entanglement uses Tree-sitter to measure source code.

Build the program with the stable Rust toolchain:

```sh
cargo build --release
```

Run tests with `cargo test --locked`.

Set `SOURCE_FILE` to a source file supported by a registered grammar, then use
one of these commands:

```sh
entanglement file "$SOURCE_FILE"
entanglement repo src --format json
entanglement patch "$SOURCE_FILE" --diff change.patch
entanglement candidate . --diff change.patch --format json
entanglement candidate "$SOURCE_FILE" --diff change.patch
git diff -- "$SOURCE_FILE" | entanglement candidate . --diff -
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

For a `candidate` file input, use the nearest ancestor directory containing a
recognized project manifest or `.git` to resolve code references. If there is
no such directory, use the root directory from the diff path.

Supply a unified diff with `--diff PATH`. Use `--diff -` to read the diff from
standard input. For a directory input, diff paths are relative to that
directory. For a file input, the diff must contain one file patch. Its old
path must match the selected file. The `patch` and `candidate` commands do
not write source files.

| Metric | Counting rule in Entanglement | Paper or publication |
| --- | --- | --- |
| NLOC (non-comment lines of code) | Count each source line covered by a code token once. Exclude comments and syntax that belongs to an unsupported embedded language. | Robert E. Park, [*Software Size Measurement: A Framework for Counting Source Statements*](https://www.sei.cmu.edu/library/software-size-measurement-a-framework-for-counting-source-statements/), CMU/SEI-92-TR-020 (1992). Defines a framework for physical source-line counting rules. Entanglement uses the rule in this row. |
| Halstead metrics | Count Tree-sitter Rust terminals as operators when they are keywords, punctuation, or operator symbols; count identifiers, literals, and lifetime or label spellings as operands. Exact spellings define distinct operators and operands. Ignore comments and whitespace. Report n1, n2, N1, N2, vocabulary, length, estimated length, volume, difficulty, effort, time, program level, and estimated bugs per file and function. Empty inputs produce finite zero values. | Maurice H. Halstead, [*Elements of Software Science*](https://doi.org/10.1016/C2013-0-04680-3), Elsevier (1977). |
| Cyclomatic complexity (CC) | Start each function at 1. Add 1 for each control-flow decision and logical condition. For a captured multiway decision with N cases, add `max(N - 1, 0)`. | [NIST SP 500-235, *Structured Testing: A Testing Methodology Using the Cyclomatic Complexity Metric*](https://nvlpubs.nist.gov/nistpubs/Legacy/SP/nistspecialpublication500-235.pdf) (1996), sections 2.2 and 4.1. Defines the metric and decision-counting method. |
| Cognitive complexity (CogC) | Start each function at 0. Each captured conditional (including a conditional binding), loop, or multiway decision adds 1 plus the current nesting depth. Alternate and chained branches add 1 without a nesting surcharge. Count a multiway decision once, with guarded cases treated as nested conditionals. Closures and nested functions add nesting for their contents; asynchronous blocks do not. Add 1 for the first logical AND or OR operator in a sequence, then for each change between operator kinds; parentheses keep a sequence together and negation splits it without adding a point. Labeled loop jumps add 1. Each function in a detected direct or mutual call cycle gets 1 recursion point; ordinary calls are free. | SonarSource, [*Cognitive Complexity*](https://www.sonarsource.com/docs/CognitiveComplexity.pdf). Entanglement maps its flow-break, nesting, and logical-sequence principles to captured syntax. |

Use `--format human` for a terminal report. This is the default format.
Use `--format json` for a JSON document. You can put `--format` before or
after the command.

JSON includes file and function Halstead indicators, CC and CogC function
contributions, and ranges for embedded languages. Patch and candidate reports
include before/after/delta values for each Halstead indicator, plus added and
removed token counts with token kind, spelling, and source line. Token changes
explain contributors; volume and other derived values are recalculated from
the full token set and are not additive. Patch comparisons also include
separate CC and CogC contribution changes. JSON includes code reference
results when applicable.

| Code reference state | Meaning |
| --- | --- |
| `exact` | One local symbol matches the reference. |
| `ambiguous` | More than one local symbol matches the reference. This can occur when module contexts resolve the same name to different symbols. |
| `external` | The reference names a symbol outside the analyzed files. |
| `unresolved` | The resolver cannot find a valid target. References that need type inference or method dispatch have this state. Different results from module contexts can also cause this state. If independent module roots declare the same source file, references from that file to other files have this state. The resolver does not select one root. |
