# Entanglement

Entanglement uses Tree-sitter to measure source code. It reports code size,
complexity, Halstead metrics, and maintainability. It can compare these metrics
before and after a patch without changing source files.

## Build and run

Use the stable Rust toolchain to build the program:

```sh
cargo build --release
```

The executable is `target/release/entanglement`. The examples below assume it
is on your `PATH`. Run tests with `cargo test --locked`.

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
and follows symbolic links. Entanglement analyzes each file once. It keeps
all discovered paths to resolve module references. Broken links and directory
link cycles cause an error.

Use `--diff PATH` to supply a unified diff. Use `--diff -` to read it from
standard input. For a directory input, diff paths must be relative to that
directory. For a file input, the diff must contain one file patch. The old
path must match the selected file. Both `patch` and `candidate` apply changes
in memory. They do not write source files.

The `candidate` command includes old files named in the diff, even if Git
ignore rules exclude them. For a file input, it finds the nearest parent
that contains `.git` or a supported project manifest. It uses that directory
to resolve references. If none exists, it uses the root directory from the
diff path.

Supported manifests are `Cargo.toml`, `package.json`, `tsconfig.json`,
`jsconfig.json`, `pyproject.toml`, `setup.py`, `setup.cfg`, and `go.mod`.

## Languages

Entanglement supports Rust, Python, Go, TypeScript, TSX, JavaScript, Svelte, HTML,
and CSS.
Language descriptors select the grammar and analyzer.

Python supports `.py`, `.pyi`, and files without an extension that have a
recognized Python 3 shebang. Functions, async functions, generators, methods,
and lambdas use the shared function metrics. Comments do not count toward
token metrics. Docstrings count as string literals.

TypeScript supports `.ts`, `.mts`, and `.cts`. TSX supports `.tsx`.
JavaScript supports `.js`, `.jsx`, `.mjs`, and `.cjs`. JSX expressions and
callbacks use the metrics of the JavaScript or TSX file that contains them.
Interfaces and type declarations do not count as runtime functions.

Svelte supports `.svelte`. The instance `<script>` block uses TypeScript
when `lang="ts"` and JavaScript otherwise; `<script module>` keeps a
separate module execution context. `<style>` uses CSS. Template expressions,
such as `{#if ready}`, `{item.name}`, `onclick={() => save()}`, or
`{@attach focus}`, use the component's script language and share the
component lexical context.

Each component has an implicit `<component>` metric scope. `{#if}` and
`{#each}` contribute control-flow complexity to the smallest enclosing
component or snippet scope, and `{#await}` is counted as a multiway decision.
`{#snippet}` declarations are function scopes. JavaScript or TypeScript
callbacks inside markup remain separate functions, so their decisions are not
also charged to the component. Template bindings from each/await/snippet
constructs shadow outer names in their lexical ranges. Component tags resolve
through normal file-module imports, and every `.svelte` file exposes a
synthetic default component export.

### Embedded languages

Entanglement uses registered grammars to analyze embedded code. It keeps
expressions in the surrounding language. For example, `${...}` inside an
HTML tagged template still contributes to JavaScript or TypeScript metrics.

HTML `<script>` blocks use JavaScript by default. A TypeScript `lang`
attribute or MIME type selects TypeScript. `type="module"` selects
JavaScript. HTML `<style>` blocks use CSS. JavaScript and TypeScript tagged
templates select a grammar by tag name, such as `html` or `css`.

Python uses the call name to select a registered grammar. The call must have
one direct string literal argument, such as `html(f"<p>{value}</p>")` or
`tools.css(r"a { color: red }")`. Literal text uses the embedded language.
F-string replacement fields remain Python. HTML `<script lang="python">`
and `<script type="python">` blocks also select Python.

Python templates require one plain, raw, or f-string literal. Adjacent
strings, bytes literals, variables, multiple arguments, and keyword
arguments do not select an embedded language. Ordinary strings and unrelated
calls, such as `print("<p>")`, remain Python.

The JSON `injections` list gives source byte ranges and their analysis state.
Unknown embedded languages have `analyzed: false`. Python strings also have
this state when non-raw escapes, doubled braces, or f-string conversions or
format specifiers prevent a safe mapping from runtime text to source ranges.

Go supports `.go`. Functions, methods, and function literals use the shared
function metrics. A function literal assigned to one name takes that name.
Each `switch`, type switch, and `select` adds one decision per non-default
case. An unconditional `for`, including `for ;;` and headers with no condition,
adds loop nesting to cognitive complexity but no CC decision. Empty and
default-only switches and selects add no CC decision. Labeled `break` and `continue` and every
`goto` add a labeled-jump point.

### Python comprehension complexity

Comprehension `for` and filter clauses add cognitive complexity in source
order. Each clause nests under the previous clause. Under this convention,
one generator with one filter has complexity 3. Two generators with one
filter have complexity 6.

### Resolve code references

The JavaScript and TypeScript resolver follows relative imports to analyzed
files. It supports file extensions, `index` files, named and default imports,
namespaces, aliases, and re-exports. Bare package imports are external.
Subpath imports that start with `#`, such as SvelteKit's `#lib/*`, resolve
through the `imports` field of the nearest `package.json`. Conditional
targets use the `types`, `import`, and `default` conditions. A target that
names a package is external. References that need `tsconfig` path aliases,
the SvelteKit 2 `$lib` alias, package export maps, type inference, or
dynamic method dispatch remain unresolved.

The Python resolver follows dotted and package-relative imports under a
known project root or its `src` directory. It supports `.py`, `.pyi`, package
`__init__.py` files, aliases, and re-exports. Explicit imports can include
names that start with an underscore. Bare imports without a known local
target are external. Missing local modules remain unresolved.

Python references remain unresolved when an exact target cannot be proved.
This includes dynamic imports, dynamic `__all__`, uncertain star imports,
class or instance method dispatch, and `global` or `nonlocal` rebinding.
Assignments hide imports with the same name throughout a function.
A module assignment can invalidate an imported binding used by a function,
even if the function appears before the assignment. Comprehension targets
stay in their own scope.

The Go resolver treats the files in one directory with the same package
clause as one package. Import paths resolve through the module path in the
nearest `go.mod`. Exported names resolve across packages. Unexported names
resolve only inside their package. Import paths outside the module and
predeclared identifiers such as `len` are external. Function parameters and
block-scoped declarations shadow package names from the end of their
declaration. Ordinary parameters and receivers bind in the function body;
type parameters also cover the appropriate declaration signature. A type-switch
alias binds separately after each case's type list. Parenthesized direct calls
and generic calls resolved to functions participate in recursion detection.

Import strings and quoted module paths are decoded before lookup. The nearest
`go.mod` is a boundary even when it is malformed or unreadable. Imports do not
cross into a nested module merely because its directory exists. An unavailable
unnamed external import uses a heuristic binding from its last path element,
after a `/vN` major version suffix is removed; this never proves an exact symbol.

Go references remain unresolved when the target depends on types. This
includes method calls, field access, and dot imports. Bare composite-literal
keys are omitted when their meaning requires type information.

All requested Go files contribute source metrics; no GOOS, GOARCH, or build-tag
configuration is selected. Test-only declarations are visible to same-package
tests but not to production files or imports. A `_test` package-name suffix alone
does not classify a source file as a test. Unique declarations in platform-named,
build-constrained, or cgo files remain unresolved; duplicate candidate declarations
remain ambiguous. This is a source inventory, not a type-checked build.

Logical paths naming one physical file must agree on resolution. Separate Go
injections do not share package members or imports. The grammar metadata also
recognizes `go` in embedded-language labels. Workspace, replacement, vendoring,
and dependency-graph selection are not implemented.

Candidate analysis retains a run-local configuration snapshot. Changes to `go.mod`,
including creation and deletion, affect the after-state without writing to disk.
Unchanged observed manifest contents are shared by value across the before and
after snapshots, not reread from the live filesystem.

## Select metrics

Use `--metrics` before or after any command. Supply a comma-separated list:
`nloc`, `cc`, `density`, `cogc`, `halstead`, or `mi`. Repeat the option to add
metrics. The default is all metrics, also available as `--metrics all`.
NLOC is always included. Unknown names and empty values cause an error.

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

Some metrics need other metrics as inputs. `density` needs CC. `mi` needs
Halstead volume and CC. These inputs appear in separate report sections only
if you select them. The MI report always includes its inputs and score effects.
CogC includes recursion results. The `repo` and `candidate` commands analyze
references across the repository to detect recursion.

| Metric | Counting rule in Entanglement | Paper or publication |
| --- | --- | --- |
| NLOC (non-comment lines of code) | Count each source line covered by a code token once. Exclude comments and syntax that belongs to an unsupported embedded language. | Robert E. Park, [*Software Size Measurement: A Framework for Counting Source Statements*](https://www.sei.cmu.edu/library/software-size-measurement-a-framework-for-counting-source-statements/), CMU/SEI-92-TR-020 (1992). Defines a framework for physical source-line counting rules. Entanglement uses the rule in this row. |
| Halstead metrics | Each language descriptor classifies Tree-sitter terminals as operators or operands. Exact spellings define distinct operators and operands. Ignore comments and whitespace. Report n1, n2, N1, N2, vocabulary, length, estimated length, volume, difficulty, effort, time, program level, and estimated bugs per file and function. Empty inputs produce finite zero values. | Maurice H. Halstead, [*Elements of Software Science*](https://doi.org/10.1016/C2013-0-04680-3), Elsevier (1977). |
| Maintainability index (MI) | `clamp((171 - 5.2 ln(V) - 0.23 CC - 16.2 ln(NLOC)) × 100 / 171, 0, 100)`, where V is Halstead volume and CC is cyclomatic complexity. Log inputs use `max(value, 1)`. File CC is the sum of function CC; function MI uses its own CC and NLOC. Bands are 0–<10 red/low, 10–<20 yellow/moderate, and 20–100 green/good. Higher scores indicate better maintainability. | Oman and Hagemeister, [*Metrics for Assessing a Software System's Maintainability*](https://doi.org/10.1109/ICSM.1992.242525), ICSM (1992); [Microsoft Code Metrics: Maintainability Index](https://learn.microsoft.com/en-us/visualstudio/code-quality/code-metrics-maintainability-index-range-and-meaning). |
| Cyclomatic complexity (CC) | Start each function at 1. Add 1 for each control-flow decision and logical condition. For a captured multiway decision with N cases, add `max(N - 1, 0)`. | [NIST SP 500-235, *Structured Testing: A Testing Methodology Using the Cyclomatic Complexity Metric*](https://nvlpubs.nist.gov/nistpubs/Legacy/SP/nistspecialpublication500-235.pdf) (1996), sections 2.2 and 4.1. Defines the metric and decision-counting method. |
| Cognitive complexity (CogC) | Start each function at 0. Each captured conditional (including a conditional binding), loop, or multiway decision adds 1 plus the current nesting depth. Alternate and chained branches add 1 without a nesting surcharge. Count a multiway decision once, with guarded cases treated as nested conditionals. Closures and nested functions add nesting for their contents; asynchronous blocks do not. Add 1 for the first logical AND or OR operator in a sequence, then for each change between operator kinds; parentheses keep a sequence together and negation splits it without adding a point. Labeled loop jumps add 1. Each function in a detected direct or mutual call cycle gets 1 recursion point; ordinary calls are free. | SonarSource, [*Cognitive Complexity*](https://www.sonarsource.com/docs/CognitiveComplexity.pdf). Entanglement maps its flow-break, nesting, and logical-sequence principles to captured syntax. |

## Read reports

Use `--format human` for a terminal report. This is the default.
Use `--format json` for a JSON document. The option can appear before or
after the command.

With all metrics selected, JSON includes file and function Halstead and MI
values, MI bands, CC and CogC contributions, and embedded-language ranges.
If you select fewer metrics, the report omits the other metric fields.

Patch and candidate reports include values before and after the change,
their differences, and added or removed token and CC contributors. They show
how volume, CC, NLOC, and clamping affect MI. A positive MI change indicates
better maintainability. A missing file or function has no invented MI score.

Function changes include `before_range` and `after_range`. Each range has
`start_byte` and `end_byte`: the start is inclusive and the end is exclusive.
A missing function side is `null`. JSON also includes code reference results
where applicable.

| Code reference state | Meaning |
| --- | --- |
| `exact` | One local symbol matches the reference. |
| `ambiguous` | More than one local symbol matches the reference. This can occur when module contexts resolve the same name to different symbols. |
| `external` | The reference names a symbol outside the analyzed files. |
| `unresolved` | The resolver cannot find a valid target. References that need type inference or method dispatch have this state. Different results from module contexts can also cause this state. If independent module roots declare the same source file, references from that file to other files have this state. The resolver does not select one root. |

## Add a language

New languages use the shared commands, metrics, and reports.

1. Add the Tree-sitter grammar dependency and assets.
2. Add a language descriptor. Include the grammar scope and name, file
   selectors, syntax query, token classification, injection rules, and
   project manifests. Add a resolution family if local references need it.
3. Map grammar captures to shared function, decision, logical-operator,
   reference, and injection roles. The common analyzer uses these roles.
4. Add a small adapter only if the grammar needs syntax normalization.
5. Add fixtures for native syntax and embedded code. Check which language
   owns each range. Run `entanglement repo` and `entanglement candidate`
   to evaluate the fixtures.
