# Entanglement

Entanglement uses Tree-sitter to measure source code. It reports code size,
complexity, Halstead metrics, and maintainability. It can compare these metrics
before and after a patch without changing source files.

## Install, build, and run

Use the stable Rust toolchain to install from GitHub:

```sh
cargo install --git https://github.com/alexandreteles/entanglement.git --branch main --locked
```

This builds in release mode and installs the executable in `~/.cargo/bin`.
Make sure that directory is on your `PATH`. From a local checkout, use
`cargo install --path . --locked` instead.

To build without installing:

```sh
cargo build --release
```

The executable is `target/release/entanglement`. If you have not installed it,
use that path in place of `entanglement` in the examples below. Run tests with
`cargo test --locked`.

Set `SOURCE_FILE` to a supported source file. Then select a command:

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

### File search and patch input

File search uses Git ignore rules. It includes hidden files, skips `.git`,
and follows symbolic links. Broken links and directory link cycles cause an
error.

Use `--diff PATH` to supply a unified diff. Use `--diff -` to read it from
standard input. For a directory input, diff paths must be relative to that
directory. For a file input, the diff must contain one file patch. The old
path must match the selected file.

The `candidate` command includes old files named in the diff, even if Git
ignore rules exclude them. For a file input, it finds the nearest parent
that contains `.git` or a supported project manifest. It uses that directory
to resolve references. If none exists, it uses the root directory from the
diff path.

Supported manifests are `Cargo.toml`, `package.json`, `tsconfig.json`,
`jsconfig.json`, `pyproject.toml`, `setup.py`, `setup.cfg`, and `go.mod`.

## Languages

- Rust
- Python
- Go
- TypeScript
- TSX
- JavaScript
- Astro
- Svelte
- HTML
- CSS

## Select metrics

Use `--metrics` before or after any command. Supply a comma-separated list:
`nloc`, `cc`, `density`, `cogc`, `halstead`, or `mi`. Repeat the option to add
metrics. The default is all metrics, also available as `--metrics all`.
NLOC is always included.

The CLI accepts these alternative names:

- `cc`: `cyclomatic` or `cyclomatic-complexity`.
- `density`: `cyclomatic-density`.
- `cogc`: `cognitive` or `cognitive-complexity`.
- `mi`: `maintainability` or `maintainability-index`.

```sh
entanglement --metrics=cogc repo src --format json
entanglement candidate . --diff change.patch --metrics cc,halstead --format json
entanglement --metrics mi --metrics density file "$SOURCE_FILE"
```

`density` needs CC. `mi` needs Halstead volume and CC. These inputs appear in
separate report sections only if you select them. The MI report always includes
its inputs and score effects.
The `repo` and `candidate` commands analyze references across the repository
to detect recursion.

| Metric | Counting rule in Entanglement | Paper or publication |
| --- | --- | --- |
| NLOC (non-comment lines of code) | Count each source line covered by a code token once. Exclude comments and syntax that belongs to an unsupported embedded language. | Robert E. Park, [*Software Size Measurement: A Framework for Counting Source Statements*](https://www.sei.cmu.edu/library/software-size-measurement-a-framework-for-counting-source-statements/), CMU/SEI-92-TR-020 (1992). |
| Halstead metrics | Each language descriptor classifies Tree-sitter terminals as operators or operands. Exact spellings define distinct operators and operands. Ignore comments and whitespace. Report n1, n2, N1, N2, vocabulary, length, estimated length, volume, difficulty, effort, time, program level, and estimated bugs per file and function. | Maurice H. Halstead, [*Elements of Software Science*](https://doi.org/10.1016/C2013-0-04680-3), Elsevier (1977). |
| Maintainability index (MI) | `clamp((171 - 5.2 ln(V) - 0.23 CC - 16.2 ln(NLOC)) × 100 / 171, 0, 100)`, where V is Halstead volume and CC is cyclomatic complexity. Log inputs use `max(value, 1)`. File CC is the sum of function CC; function MI uses its own CC and NLOC. Bands are 0–<10 red/low, 10–<20 yellow/moderate, and 20–100 green/good. Higher scores indicate better maintainability. | Oman and Hagemeister, [*Metrics for Assessing a Software System's Maintainability*](https://doi.org/10.1109/ICSM.1992.242525), ICSM (1992); [Microsoft Code Metrics: Maintainability Index](https://learn.microsoft.com/en-us/visualstudio/code-quality/code-metrics-maintainability-index-range-and-meaning). |
| Cyclomatic complexity (CC) | Start each function at 1. Add 1 for each control-flow decision and logical condition. For a captured multiway decision with N cases, add `max(N - 1, 0)`. | [NIST SP 500-235, *Structured Testing: A Testing Methodology Using the Cyclomatic Complexity Metric*](https://nvlpubs.nist.gov/nistpubs/Legacy/SP/nistspecialpublication500-235.pdf) (1996), sections 2.2 and 4.1. |
| Cognitive complexity (CogC) | Start each function at 0. Each captured conditional (including a conditional binding), loop, or multiway decision adds 1 plus the current nesting depth. Alternate and chained branches add 1 without a nesting surcharge. Count a multiway decision once, with guarded cases treated as nested conditionals. Closures and nested functions add nesting for their contents; asynchronous blocks do not. Add 1 for the first logical AND or OR operator in a sequence, then for each change between operator kinds; parentheses keep a sequence together and negation splits it without adding a point. Labeled loop jumps add 1. Each function in a detected direct or mutual call cycle gets 1 recursion point; ordinary calls are free. | SonarSource, [*Cognitive Complexity*](https://www.sonarsource.com/docs/CognitiveComplexity.pdf). |

## Read reports

Use `--format human` for a terminal report. This is the default.
Use `--format json` for a JSON document. The option can appear before or
after the command.

With all metrics selected, JSON includes file and function Halstead and MI
values, MI bands, CC and CogC contributions, and embedded-language ranges.

Patch and candidate reports include values before and after the change,
their differences, and added or removed token and CC contributors. They show
how volume, CC, NLOC, and clamping affect MI.

Function changes include `before_range` and `after_range`. Each range has
`start_byte` and `end_byte`: the start is inclusive and the end is exclusive.
A missing function side is `null`.

| Code reference state | Meaning |
| --- | --- |
| `exact` | One local symbol matches the reference. |
| `ambiguous` | More than one local symbol matches the reference. |
| `external` | The reference names a symbol outside the analyzed files. |
| `unresolved` | The resolver cannot find a valid target. References that need type inference or method dispatch have this state. |

## Add a language

1. Add the Tree-sitter grammar dependency and assets.
2. Add a language descriptor. Include the grammar scope and name, file
   selectors, syntax query, token classification, injection rules, and
   project manifests. Add a resolution family if local references need it.
3. Map grammar captures to shared function, decision, logical-operator,
   reference, and injection roles.
4. Add a small adapter only if the grammar needs syntax normalization.
5. Add fixtures for native syntax and embedded code. Check which language
   owns each range. Run `entanglement repo` and `entanglement candidate`
   to evaluate the fixtures.
