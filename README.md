# Entanglement

Entanglement measures Rust source code with Tree-sitter. The architecture is
specified in [ARCHITECTURE.md](ARCHITECTURE.md).

Build the program with the stable Rust toolchain:

```sh
cargo build --release
```

The build embeds the upstream Rust grammar assets selected by Cargo.
Tree-sitter loads these assets from its cache. The first analysis requires a
C compiler to build the grammar library. Later runs use the cached library.
The program does not require the source repository or Cargo cache at runtime.
Tree-sitter 0.27 selects languages by content through file-based APIs. Virtual
patches use Rust file metadata. A future grammar that selects its language by
content will need a virtual-source adapter for those APIs.

Use one of these commands:

```sh
entanglement file src/main.rs
entanglement repo src --format json
entanglement patch src/main.rs --diff change.patch
entanglement candidate . --diff change.patch --format json
entanglement candidate src/main.rs --diff change.patch
git diff -- src/main.rs | entanglement candidate . --diff -
```

`file` analyzes one complete file. `repo` discovers regular files below a
directory and analyzes files with a registered grammar in parallel.
Discovery does not follow symbolic links.

`patch` applies one unified diff to a selected source file in memory.
`candidate` applies a unified diff to a directory or selected file in memory.
A file candidate uses the nearest Cargo or Git repository for resolution.
If neither exists, it uses the directory implied by the diff path.
Directory diff paths are relative to the selected directory. File diff paths
must match the selected file. Use `--diff -` to read standard input.
Neither command writes source files.

Unified diffs can contain several hunks and files. They can create, delete,
or rename files. Hunk context must match the source. Paths must stay below
the selected root and must not contain symbolic links. Binary patches are
not supported. Invalid input produces an error and a failed exit status.

NLOC counts rows with non-comment terminal syntax. Complexity starts at one
for each function and adds control-flow decisions. Each increment retains its
kind, byte range, and source line. Complexity density is complexity divided
by `max(NLOC, 1)`. Patch reports use complete before and after scores.

Only Rust is registered. Macro token trees use the standard Rust injection
query. The `v!` macro has an HTML override. An injection without a registered
grammar has `analyzed: false`; its syntax does not count as parent-language
code.

JSON contains file metrics, function contributions, injection ranges,
reference-resolution results, and patch comparisons when applicable.
Resolution reports exact, ambiguous, external, or unresolved references.
References that need type inference or trait dispatch remain unresolved.
If several crate roots declare the same source file, cross-file references
from that file remain unresolved. The resolver does not select one crate.
