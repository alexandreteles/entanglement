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
manifest declared by a registered language or `.git` to resolve code
references. If there is no such directory, use the root directory from the
diff path. The current descriptors recognize `Cargo.toml`, `package.json`,
`tsconfig.json`, `jsconfig.json`, `pyproject.toml`, `setup.py`, and
`setup.cfg`.

Supply a unified diff with `--diff PATH`. Use `--diff -` to read the diff from
standard input. For a directory input, diff paths are relative to that
directory. For a file input, the diff must contain one file patch. Its old
path must match the selected file. The `patch` and `candidate` commands do
not write source files.

## Languages and embedded code

Entanglement selects grammars and analyzers from language descriptors. The
current registry includes Rust, Python, TypeScript, TSX, JavaScript, HTML,
and CSS. Python accepts `.py` and `.pyi`; function, async-function,
generator, method, and lambda bodies use the shared function metrics.
Extensionless scripts with a recognized Python 3 shebang are also selected.
TypeScript accepts `.ts`, `.mts`, and `.cts`; TSX accepts `.tsx`; JavaScript
accepts `.js`, `.jsx`, `.mjs`, and `.cjs`. JSX stays part of its JavaScript or
TSX syntax tree, so expressions and callback functions inside React or Solid
components are measured as host-language code. Interfaces and type
declarations are syntax, not runtime functions.

HTML `<script>` blocks default to JavaScript. A TypeScript `lang` attribute or
TypeScript MIME type selects TypeScript, while `type="module"` remains
JavaScript. HTML `<style>` blocks select CSS. JavaScript and TypeScript tagged
templates select a registered embedded grammar by the tag name; the `html` and
`css` tags therefore analyze their template bodies. Host expressions inside
`${...}` remain part of the JavaScript or TypeScript function and its metrics.
The JSON `injections` list reports source byte ranges and whether an analyzer
handled each range. An unknown injected language remains visible there with
`analyzed: false`.

Python can embed a registered language through one direct string-literal
argument to a call whose simple callee matches that language label, such as
`html(f"<p>{value}</p>")` or `tools.css(r"a { color: red }")`. Literal source
stays in the guest language, while f-string replacement fields stay Python
and retain their host metrics. HTML `<script lang="python">` and
`<script type="python">` blocks select the same registered analyzer. A single
plain, raw, or f-string literal is required; adjacent strings, bytes literals,
variables, multiple arguments, and keyword arguments are not inferred as
templates. Non-raw escapes, doubled-brace escapes, or f-string conversion/format
specifiers are reported unanalyzed because their runtime text cannot be mapped
safely to source ranges. Ordinary strings and unrelated calls such as
`print("<p>")` remain Python. Comments are excluded from token metrics, while
docstrings count as ordinary string literals.
Comprehension `for` and filter clauses contribute cognitive complexity in
source order, with each clause nested under the preceding clause. This is
Entanglement's evaluation-order convention: one generator with one filter has
cognitive complexity 3, and two generators with one filter have complexity 6.

To add a language, add its Tree-sitter grammar dependency and assets, then add
a descriptor with the grammar scope and name, file selectors, shared syntax
query, token classification, injection rules, project manifests, and a
resolution family when the language has local references to resolve. Shared
queries map grammar captures to common function, decision, logical-operator,
reference, and injection roles. The common analyzer consumes those roles for
NLOC, complexity, Halstead, maintainability, injections, and reports. A small
language adapter is appropriate only when the grammar needs syntax-specific
normalization; adding a language should not require new command, metric, or
report branches. Add fixtures for native syntax and mixed-language ownership,
then run `entanglement repo` and `entanglement candidate` on them.

The JavaScript and TypeScript resolver follows local relative imports that
resolve to analyzed files, including supported extension and `index` forms,
and handles local named, default, namespace, alias, and re-export references.
Bare package imports are external. It does not apply `tsconfig` path aliases,
package export maps, type inference, or dynamic method dispatch; references
that need those features stay unresolved.

The Python resolver follows dotted and package-relative imports under an
established project root or its `src` directory. It recognizes `.py`,
`.pyi`, package `__init__.py` files, explicit aliases, and package
re-exports; explicit imports of underscore-prefixed names are allowed.
Bare imports without an indexed local target are external; missing project-local
modules remain unresolved. Dynamic imports, dynamic `__all__`, star imports
whose targets cannot be proved, class or instance method dispatch, and effects
that depend on `global` or `nonlocal` rebinding remain unresolved when the
target cannot be established exactly.
Assignments shadow same-named imports throughout their function. A module-level
assignment can invalidate a same-module imported binding, including in earlier
deferred function bodies; comprehension targets stay in their own scope.

## Selecting metrics

All commands accept the global `--metrics` option before or after the command.
Its value is a comma-separated list of canonical names: `nloc`, `cc`,
`density`, `cogc`, `halstead`, and `mi`. Repeat the option to add more
metrics. With no option, all metrics are reported; `--metrics all` selects the
same set. NLOC is always included, even when it is not named. Unknown names
and empty values are errors.

The CLI also accepts `cyclomatic` and `cyclomatic-complexity` for `cc`,
`cyclomatic-density` for `density`, `cognitive` and
`cognitive-complexity` for `cogc`, and `maintainability` and
`maintainability-index` for `mi`.

```sh
entanglement --metrics=cogc repo src --format json
entanglement candidate . --diff change.patch --metrics cc,halstead --format json
entanglement --metrics mi --metrics density file "$SOURCE_FILE"
```

Selecting `density` calculates cyclomatic complexity as an input, but hides
the separate CC values and contributors unless `cc` is also selected.
Selecting `mi` calculates Halstead volume and cyclomatic complexity as inputs,
but hides their separate report sections unless `halstead` or `cc` is selected.
The MI report retains its own input and score-effect details. Selecting
`cogc` still includes recursion results, which require repository-wide analysis
for repository and candidate commands.

| Metric | Counting rule in Entanglement | Paper or publication |
| --- | --- | --- |
| NLOC (non-comment lines of code) | Count each source line covered by a code token once. Exclude comments and syntax that belongs to an unsupported embedded language. | Robert E. Park, [*Software Size Measurement: A Framework for Counting Source Statements*](https://www.sei.cmu.edu/library/software-size-measurement-a-framework-for-counting-source-statements/), CMU/SEI-92-TR-020 (1992). Defines a framework for physical source-line counting rules. Entanglement uses the rule in this row. |
| Halstead metrics | Each language descriptor classifies Tree-sitter terminals as operators or operands. Exact spellings define distinct operators and operands. Ignore comments and whitespace. Report n1, n2, N1, N2, vocabulary, length, estimated length, volume, difficulty, effort, time, program level, and estimated bugs per file and function. Empty inputs produce finite zero values. | Maurice H. Halstead, [*Elements of Software Science*](https://doi.org/10.1016/C2013-0-04680-3), Elsevier (1977). |
| Maintainability index (MI) | `clamp((171 - 5.2 ln(V) - 0.23 CC - 16.2 ln(NLOC)) × 100 / 171, 0, 100)`, where V is Halstead volume and CC is cyclomatic complexity. Log inputs use `max(value, 1)`. File CC is the sum of function CC; function MI uses its own CC and NLOC. Bands are 0–<10 red/low, 10–<20 yellow/moderate, and 20–100 green/good. Higher scores indicate better maintainability. | Oman and Hagemeister, [*Metrics for Assessing a Software System's Maintainability*](https://doi.org/10.1109/ICSM.1992.242525), ICSM (1992); [Microsoft Code Metrics: Maintainability Index](https://learn.microsoft.com/en-us/visualstudio/code-quality/code-metrics-maintainability-index-range-and-meaning). |
| Cyclomatic complexity (CC) | Start each function at 1. Add 1 for each control-flow decision and logical condition. For a captured multiway decision with N cases, add `max(N - 1, 0)`. | [NIST SP 500-235, *Structured Testing: A Testing Methodology Using the Cyclomatic Complexity Metric*](https://nvlpubs.nist.gov/nistpubs/Legacy/SP/nistspecialpublication500-235.pdf) (1996), sections 2.2 and 4.1. Defines the metric and decision-counting method. |
| Cognitive complexity (CogC) | Start each function at 0. Each captured conditional (including a conditional binding), loop, or multiway decision adds 1 plus the current nesting depth. Alternate and chained branches add 1 without a nesting surcharge. Count a multiway decision once, with guarded cases treated as nested conditionals. Closures and nested functions add nesting for their contents; asynchronous blocks do not. Add 1 for the first logical AND or OR operator in a sequence, then for each change between operator kinds; parentheses keep a sequence together and negation splits it without adding a point. Labeled loop jumps add 1. Each function in a detected direct or mutual call cycle gets 1 recursion point; ordinary calls are free. | SonarSource, [*Cognitive Complexity*](https://www.sonarsource.com/docs/CognitiveComplexity.pdf). Entanglement maps its flow-break, nesting, and logical-sequence principles to captured syntax. |

Use `--format human` for a terminal report. This is the default format.
Use `--format json` for a JSON document. You can put `--format` before or
after the command.

With all metrics selected, JSON includes file and function Halstead and MI
values, MI band boundaries, CC and CogC function contributions, and ranges for
embedded languages. Patch and candidate reports include Halstead and MI
before/after/deltas, MI score effects by volume, CC, NLOC, and clamping, plus
added and removed token and CC contributors. Selecting a subset omits unselected
metric fields. A missing file or function side has no synthetic MI score.
Positive MI change means improved maintainability. JSON includes code
reference results when applicable. Patch and candidate function deltas include
`before_range` and `after_range` byte spans as `{start_byte, end_byte}` objects;
`start_byte` is inclusive, `end_byte` is exclusive, and an absent function side
is `null`.

| Code reference state | Meaning |
| --- | --- |
| `exact` | One local symbol matches the reference. |
| `ambiguous` | More than one local symbol matches the reference. This can occur when module contexts resolve the same name to different symbols. |
| `external` | The reference names a symbol outside the analyzed files. |
| `unresolved` | The resolver cannot find a valid target. References that need type inference or method dispatch have this state. Different results from module contexts can also cause this state. If independent module roots declare the same source file, references from that file to other files have this state. The resolver does not select one root. |
