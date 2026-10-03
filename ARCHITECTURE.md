# Initial architecture

Build a small Rust code-metrics analyzer around this pipeline:

```text
input
  ↓
file discovery / candidate patch application
  ↓
Tree-sitter language detection
  ↓
Rayon: one file per parallel task
  ↓
Tree-sitter parse
  ↓
Tree-sitter injection discovery
  ↓
one fused syntax-tree/query traversal per language tree
  ↓
normalized syntax events
  ↓
generic metrics
  ↓
cross-file name resolution
  ↓
AnalysisResult
  ↓
human renderer OR JSON renderer
```

Tree-sitter should own everything it already knows how to do. Do not implement separate lexers, byte scanners, extension tables, comment parsers, language classifiers, or language-specific metric algorithms.

Start with Rust so the tool can analyze its own code, and three metrics:

- NLOC: source lines containing non-comment code.
- Cyclomatic complexity.
- Cyclomatic complexity density: cyclomatic complexity relative to NLOC.

Retain individual cyclomatic-complexity contributions so patch analysis can explain why a score increased or decreased.

## Dependencies

Existing:

```toml
blake3 = "1.8.7"
clap = "4.6.7"
lipgloss = { version = "0.2.4", package = "charmed-lipgloss" }
memchr = "2.8.3"
tree-sitter = "0.27.0"
```

Also required:

```toml
rayon = "1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"

tree-sitter-loader = "0.27.0"
tree-sitter-rust = "0.24.2"
```

`tree-sitter-loader` is Tree-sitter's own grammar-loading and language-selection implementation and is also used by the Tree-sitter CLI. Tree-sitter 0.27's CLI depends on `tree-sitter-loader = 0.27.0`.

`blake3` identifies file contents. No metric implementation should scan raw bytes independently; syntax, comments, positions, and language boundaries come from Tree-sitter.

# Directory structure

```text
src/
├── main.rs
├── cli.rs
├── input.rs
├── analysis.rs
├── model.rs
├── resolver.rs
│
├── languages/
│   ├── mod.rs
│   └── rust.rs
│
├── queries/
│   └── rust.scm
│
├── metrics/
│   ├── mod.rs
│   ├── nloc.rs
│   ├── cyclomatic.rs
│   └── cyclomatic_density.rs
│
└── ui/
    ├── mod.rs
    ├── human.rs
    └── json.rs
```

Keep exactly one project-owned query file per language.

Do not duplicate standard Tree-sitter queries that the grammar already provides. `tree-sitter-rust` exposes its generated language plus its standard `TAGS_QUERY` and `INJECTIONS_QUERY`; use them directly and combine them with `queries/rust.scm` when constructing the effective query.

`rust.scm` therefore contains only the additional captures required by this analyzer: metric roles, imports/name-resolution information not already covered by tags, and application-specific injection overrides such as RsHtml.

# Core data model

Keep one normalized representation regardless of language:

```rust
#[derive(Debug, Clone, serde::Serialize)]
pub struct AnalysisResult {
    pub files: Vec<FileAnalysis>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct FileAnalysis {
    pub path: String,
    pub hash: String,
    pub language: String,

    pub nloc: usize,
    pub functions: Vec<FunctionAnalysis>,
    pub injections: Vec<InjectionAnalysis>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct FunctionAnalysis {
    pub name: String,
    pub start_byte: usize,
    pub end_byte: usize,

    pub nloc: usize,
    pub cyclomatic_complexity: usize,
    pub cyclomatic_density: f64,

    pub contributions: Vec<ComplexityContribution>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ComplexityContribution {
    pub kind: String,
    pub value: i32,
    pub start_byte: usize,
    pub end_byte: usize,
    pub line: usize,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct InjectionAnalysis {
    pub language: String,
    pub start_byte: usize,
    pub end_byte: usize,
    pub analyzed: bool,
}
```

Parsing should additionally produce internal facts for resolution:

```rust
struct FileFacts {
    path: PathBuf,
    module: ModulePath,
    definitions: Vec<Definition>,
    imports: Vec<Import>,
    references: Vec<Reference>,
    analysis: FileAnalysis,
}

enum Resolution {
    Exact(SymbolId),
    Ambiguous(Vec<SymbolId>),
    External,
    Unresolved,
}
```

Never guess when resolution is ambiguous.

# Language detection

Do not maintain our own extension-to-language mapping.

Use Tree-sitter's loader and grammar metadata. Tree-sitter language configurations support `file-types`, `first-line-regex`, and `content-regex`; `injection-regex` performs the equivalent lookup for injected-language names.

Conceptually:

```rust
let (language, configuration) =
    loader.language_configuration_for_file_name(path)?
        .or_else(/* Tree-sitter first-line/content selection */)?;
```

The exact selection flow should use the Tree-sitter loader APIs rather than reproducing their rules.

Initially only the Rust grammar is registered, so ordinary source selection will resolve Rust files. Adding another language should consist primarily of registering another Tree-sitter grammar and adding its analyzer query; the core does not acquire another extension table.

# Language implementation

`languages/rust.rs` registers the Rust grammar and creates its effective query.

The grammar already exposes:

```rust
tree_sitter_rust::LANGUAGE
tree_sitter_rust::TAGS_QUERY
tree_sitter_rust::INJECTIONS_QUERY
```

Combine those standard queries with:

```rust
const ANALYSIS_QUERY: &str =
    include_str!("../queries/rust.scm");
```

Compile the effective query once per worker/language:

```text
Tree-sitter TAGS_QUERY
+ Tree-sitter INJECTIONS_QUERY
+ queries/rust.scm
        ↓
one tree_sitter::Query
```

Tree-sitter's standard tagging vocabulary already includes captures such as `@definition.function`, `@definition.class`, and `@reference.call`; use that vocabulary instead of inventing equivalents.

Our own metric captures must likewise use a common vocabulary shared by every future language:

```text
@syntax.node

@metric.function
@metric.condition
@metric.logical_condition
@metric.multiway
@metric.case

@comment

@import.path
@import.alias

@injection.content
@injection.language
```

Tree-sitter queries support wildcard nodes, including `_`, which matches named or anonymous syntax nodes. This lets the same fused query expose syntax coverage needed by NLOC without a second source scanner.

The language-specific query answers only:

> Which Tree-sitter syntax corresponds to these generic semantic roles?

It does not implement any metric.

# Generic syntax events

Convert captures into a small language-independent representation:

```rust
enum SyntaxRole {
    Node,
    Function,

    Condition,
    LogicalCondition,
    Multiway,
    Case,

    Comment,

    Definition,
    Reference,
    Import,

    InjectionContent,
    InjectionLanguage,
}

struct SyntaxEvent {
    role: SyntaxRole,
    start_byte: usize,
    end_byte: usize,
    start_row: usize,
    end_row: usize,
    node_id: usize,
    parent_id: Option<usize>,
}
```

At startup, resolve query capture names to numeric capture IDs once. Do not compare strings in the analysis inner loop.

This is the query-based equivalent of the earlier `LanguageSpec` design:

```rust
pub struct LanguageSpec {
    pub functions: &'static [NodeKind],
    pub conditions: &'static [NodeKind],
    pub logical_conditions: &'static [NodeKind],
    pub cases: &'static [NodeKind],
    pub nesting: &'static [NodeKind],
    pub parameters: &'static [NodeKind],
}
```

The important property is unchanged: languages classify syntax; metrics consume generic categories.

# One fused traversal

For each syntax tree, run one combined `QueryCursor` traversal.

Do not separately walk the syntax tree once for NLOC, again for complexity, again for definitions, etc.

```text
Tree-sitter tree
       ↓
single effective Query
       ↓
single QueryCursor traversal
       ↓
captures / SyntaxEvents
       ├── NLOC
       ├── cyclomatic complexity
       ├── definitions/references
       ├── imports
       └── injections
```

The query may match a node under multiple roles, but the syntax tree itself is traversed once by the query machinery.

Injected trees recursively use the same process:

```text
parent Tree-sitter tree
       ↓
single traversal
       ↓
injection.content
       ↓
child Tree-sitter parser
       ↓
single traversal
       ↓
...
```

# Metrics

## NLOC

Do not scan the source text line-by-line.

Tree-sitter already provides row/column positions for every syntax node.

The combined query exposes syntax nodes and comment nodes. Derive NLOC from Tree-sitter positions:

```text
all terminal syntax nodes
        -
terminal syntax contained only in comments
        =
rows containing actual code
```

More precisely:

1. Use `@syntax.node` to receive syntax nodes.
2. Consider terminal nodes (`child_count() == 0`) as concrete syntax.
3. Use `@comment` to identify comment ranges.
4. Mark every row occupied by at least one non-comment terminal node.
5. NLOC is the number of unique marked rows.

This automatically handles:

```rust
// comment only                 // not NLOC

let x = 1;                     // NLOC

let y = 2; // trailing comment // NLOC
```

Blank lines contain no syntax nodes and therefore do not count.

A multi-line code token, such as a multi-line string, contributes each row occupied by that token.

The same generic calculation operates over either the complete language tree or the byte range belonging to a function.

Do not use `memchr`, regexes, or another lexical pass for NLOC.

Injected ranges belong to their injected language tree and must not be counted as ordinary parent-language syntax.

## Cyclomatic complexity

Implement cyclomatic complexity exactly once.

Language-specific queries classify syntax into generic control-flow roles; `metrics/cyclomatic.rs` contains no Rust-specific node names.

McCabe cyclomatic complexity measures the number of linearly independent paths through a control-flow graph. The standard formulation starts a connected function at 1 and increases complexity for control-flow decisions; an n-way decision contributes `n - 1`, and short-circuit Boolean operators count where they introduce conditional control flow.

The generic reducer should therefore look conceptually like:

```rust
let mut complexity = 1;

for event in function_events {
    match event.role {
        SyntaxRole::Condition
        | SyntaxRole::LogicalCondition => {
            complexity += 1;
        }

        SyntaxRole::Multiway => {
            let cases = cases_for(event.node_id);
            complexity += cases.saturating_sub(1);
        }

        _ => {}
    }
}
```

Every future language uses this same implementation.

The language query maps grammar syntax to the generic roles. For Rust this means classifying the control-flow equivalents of:

```text
if / if let
while / while let
for
let ... else
?
&&
||
match + match arms
match guards
```

The metric code does not know what an `if_expression`, `match_expression`, or Rust `?` node is.

For example, a future language might classify:

```text
if_statement      → @metric.condition
for_statement     → @metric.condition
while_statement   → @metric.condition
catch_clause      → @metric.condition
binary &&         → @metric.logical_condition
binary ||         → @metric.logical_condition
switch_statement  → @metric.multiway
case_statement    → @metric.case
```

while Rust maps its own grammar nodes to those exact same roles.

Store each increment as a contribution:

```rust
ComplexityContribution {
    kind,
    value,
    start_byte,
    end_byte,
    line,
}
```

Keep the function baseline as a contribution as well.

## Cyclomatic complexity density

Define one formula and keep it stable:

```rust
density =
    cyclomatic_complexity as f64
    / nloc.max(1) as f64;
```

If desired for display, multiply by 100 and describe it explicitly as complexity per 100 NLOC. Renderers must not independently calculate it.

# Language injections

Language injection is a first-class part of the parser architecture, not a later special case.

Tree-sitter explicitly models mixed-language files as a parent syntax tree plus injected syntax trees. Its standard query interface uses `@injection.content` and `@injection.language`, or an `injection.language` property, to tell the consumer which range should be reparsed with another grammar.

Processing is recursive:

```text
file
 ↓
detect parent language
 ↓
parse parent
 ↓
find @injection.content
 ↓
resolve injected language through Tree-sitter loader
 ↓
parse injected content
 ↓
find further injections
 ↓
...
```

For example, once the corresponding grammars exist:

```text
template.rs
├── Rust
└── RsHtml/HTML injection
    ├── HTML
    └── <style> → CSS
```

or conceptually:

```rust
use rshtml::{View, v};

fn main() {
    let hello = v!(<p>Hello {template}</p>);
}
```

The Rust query can recognize the relevant `v!(...)` macro token tree as an injection using standard Tree-sitter injection captures rather than treating the markup as ordinary Rust syntax.

`queries/rust.scm` may therefore contain application-specific injection rules that the generic Rust grammar cannot know about, for example conceptually:

```scheme
(
  (macro_invocation
    macro: (identifier) @_macro
    (token_tree) @injection.content)
  (#eq? @_macro "v")
  (#set! injection.language "html")
)
```

Tree-sitter's own Rust grammar already provides an injection query and currently treats macro token trees as injected Rust; use its standard query as the default and allow more-specific application queries such as RsHtml to classify known embedded languages correctly.

When an injection names another language, resolve that language through Tree-sitter's injection machinery/loader rather than an application-owned map. Tree-sitter grammar metadata provides `injection-regex` specifically for this purpose.

Initially only Rust is supported. Therefore:

- Rust syntax is fully analyzed.
- Known injected regions can still be identified.
- If their grammar is unavailable, mark them `analyzed: false`.
- Do not count unsupported injected syntax as Rust NLOC or Rust cyclomatic complexity.

When HTML, CSS, or another grammar is added later, the same recursive mechanism starts analyzing those regions without changing the metric implementations.

# Cross-file name resolution

After all files have produced `FileFacts`, build one repository-wide symbol index.

Use Tree-sitter's supplied tag query before adding equivalent patterns yourself. The current Rust grammar already extracts structs, enums, unions, type aliases, methods, functions, traits, modules, macros, calls, and implementations through its `TAGS_QUERY`.

`queries/rust.scm` should add only information still required by our resolver, particularly imports, aliases, and qualified path structure.

For the initial Rust implementation, support the minimum required intra-crate resolution:

```text
module definitions
mod foo; → foo.rs or foo/mod.rs
inline modules
functions
structs
enums
traits
type aliases
constants/statics

crate:: paths
self:: paths
super:: paths
explicit use
grouped use
use aliases
pub use
qualified paths
```

Flatten grouped imports into individual normalized imports before resolution.

Resolve in deterministic stages:

```text
1. establish crate/module paths
2. insert definitions into SymbolIndex
3. resolve imports and aliases
4. resolve qualified references
5. resolve unqualified references through lexical/module/import scope
```

Tree-sitter supplies definitions, references, imports, and syntax structure; `resolver.rs` connects them across files.

The initial resolver does not need type inference or trait/method dispatch. References requiring those semantics remain `Unresolved`.

For repository analysis, parse files independently first and perform resolution only after Rayon has returned all `FileFacts`.

For PATCH/CANDIDATE, replace the affected file's facts and update resolution using the unchanged facts from the remaining files.

# Parallelism

Use Rayon only at file-level boundaries:

```rust
let facts: Vec<FileFacts> = paths
    .par_iter()
    .map_init(
        Worker::new,
        |worker, path| worker.analyze_file(path),
    )
    .collect::<Result<_, _>>()?;
```

`Worker` owns reusable per-worker Tree-sitter state:

```rust
struct Worker {
    parsers: ParserRegistry,
    queries: QueryRegistry,
}
```

Each task performs:

```text
read one file
→ BLAKE3 hash
→ Tree-sitter language selection
→ Tree-sitter parse
→ recursively parse injections
→ one fused query traversal per tree
→ generic metrics
→ extract resolution facts
→ return FileFacts
```

Do not parallelize individual functions within a file initially.

Do not mutate the shared symbol index from Rayon workers. Collect immutable `FileFacts`, then build the index and resolve names after the parallel phase.

Rayon therefore distributes independently analyzable files through work stealing while all cross-file state remains deterministic.

# Incremental parsing

Keep each parsed tree with its source and language:

```rust
struct ParsedFile {
    source: Vec<u8>,
    language: LanguageId,
    tree: tree_sitter::Tree,
    facts: FileFacts,
}
```

PATCH and CANDIDATE must use Tree-sitter's incremental parsing support rather than discard the previous tree.

Applying a diff produces:

```rust
struct AppliedPatch {
    source: Vec<u8>,
    edits: Vec<tree_sitter::InputEdit>,
}
```

Apply each edit to the old tree and pass that edited tree back into the parser:

```rust
tree.edit(&edit);

let new_tree = parser
    .parse(&new_source, Some(&tree))
    .expect("parse failed");
```

Tree-sitter explicitly supports editing an existing tree and passing it back during reparsing so unchanged syntax can be reused.

After reparsing, rerun the one fused analysis query on the resulting tree and any affected injection trees.

# Input modes

Support four analysis modes:

```text
FILE
    Analyze one complete file.
    Tree-sitter determines its language.

REPOSITORY
    Discover files below a directory.
    Tree-sitter determines which registered grammar applies.
    Analyze supported files in parallel.
    Resolve names across files.

PATCH
    old file + unified diff
    → apply diff in memory
    → Tree-sitter incremental reparse
    → return metric deltas.

CANDIDATE
    repository/current file + proposed unified diff
    → apply diff virtually
    → incrementally update affected trees and resolution
    → return analysis without modifying disk.
```

CLI shape:

```text
tool file PATH
tool repo PATH
tool patch FILE --diff PATCH
tool candidate PATH --diff PATCH

tool ... --format human
tool ... --format json
```

`PATCH` may be a path or `-` for stdin so an agent harness can pipe a unified diff directly.

`input.rs` owns reading paths, discovery, and applying unified diffs. It does not determine programming languages. Analysis code receives source plus path/edit metadata and asks Tree-sitter to select the language.

# Patch results

For patches, expose before/after values rather than assigning an artificial "complexity of the diff":

```rust
pub struct MetricDelta<T> {
    pub before: T,
    pub after: T,
    pub delta: i64,
}
```

Example:

```json
{
  "function": "parse_expression",
  "cyclomatic_complexity": {
    "before": 8,
    "after": 11,
    "delta": 3
  }
}
```

Also compare complexity contributions:

```text
parse_expression
CC: 8 → 11 (+3)

Added contributions:
  line 42  condition          +1
  line 47  condition          +1
  line 51  logical_condition  +1
```

Removed contributions are reported identically with negative values.

The authoritative delta is always:

```text
complexity(new function) - complexity(old function)
```

Do not derive the total delta only from added diff lines.

# Output

Both renderers consume the same `AnalysisResult`.

`ui/human.rs` contains all terminal presentation. It may use Lipgloss for alignment and styling:

```text
src/parser.rs

Function             NLOC    CC    CC density
parse_expression       42     11       0.262
parse_statement        18      4       0.222
```

For patch analysis:

```text
src/parser.rs

parse_expression
  NLOC    39 → 42    (+3)
  CC       8 → 11    (+3)
  density  0.205 → 0.262

Complexity changes
  +1 condition          line 42
  +1 condition          line 47
  +1 logical_condition  line 51
```

`ui/json.rs` contains only JSON serialization:

```rust
pub fn render(result: &AnalysisResult) -> Result<String> {
    Ok(serde_json::to_string_pretty(result)?)
}
```

Never construct JSON manually and never put terminal-formatting logic into the JSON renderer.

# Minimal implementation order

Implement this vertical slice:

```text
1. Register tree-sitter-rust with Tree-sitter's language loader.
2. Let Tree-sitter select Rust automatically from grammar metadata.
3. CLI accepts one source file.
4. Parse it with Tree-sitter.
5. Build one effective Rust query from:
      tree_sitter_rust::TAGS_QUERY
    + tree_sitter_rust::INJECTIONS_QUERY
    + queries/rust.scm.
6. Run one fused QueryCursor traversal.
7. Normalize captures into SyntaxEvents.
8. Derive NLOC entirely from Tree-sitter syntax/comment positions.
9. Calculate generic cyclomatic complexity from generic control-flow roles.
10. Calculate generic CC density.
11. Store contribution ranges and lines.
12. Detect injection ranges and recursively dispatch supported injections.
13. Produce AnalysisResult.
14. Render through ui/human.rs.
15. Render through ui/json.rs.
16. Extend input to repository mode using Rayon.
17. Build the Rust module/symbol index and perform cross-file resolution.
18. Add PATCH using InputEdit + Tree-sitter incremental reparsing.
19. Add CANDIDATE as virtual PATCH application with updated resolution.
```

The implementation is correctly isolated when adding another language primarily requires registering its Tree-sitter grammar and adding one `queries/<language>.scm` file. Language detection, parsing, injections, incremental updates, NLOC extraction, metric implementations, parallel execution, and output architecture must remain generic.
