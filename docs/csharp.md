# C# analysis

Use `file` for one physical file. Use `repo` for calls across files and projects. Use `patch` for a local source change. Use `candidate` on a directory to measure a change to source, project files, or build properties. All four commands accept the same metrics as the other languages.

```sh
entanglement file src/Invoice.cs --metrics nloc,cc,cogc,halstead,mi
entanglement repo . --metrics structure --format json
entanglement candidate . --diff project-change.diff --metrics cogc,structure
```

## Source and project scope

Entanglement discovers source under the selected directory and applies the repository ignore rules. A file candidate can use a surrounding solution or repository for lookup. Each C# project supplies a separate type identity. Two projects can compile the same physical file. A repository report includes its size and function metrics once and its type evidence in each applicable project.

Project analysis reads SDK source defaults, compile items, project references, and build properties. It reads imported project files and the applicable `Directory.Build.props` and `Directory.Build.targets`. It uses the configured target frameworks and build configurations to select preprocessor branches. Source offsets in reports always refer to the input bytes. Candidate analysis reads virtual changes to these files. [MSBuild project properties](https://learn.microsoft.com/en-us/dotnet/core/project-sdk/msbuild-props) and [C# preprocessor directives](https://learn.microsoft.com/en-us/dotnet/csharp/language-reference/preprocessor-directives)

Entanglement performs static source analysis. A custom MSBuild task, a source generator, a package assembly, or runtime dispatch can supply information outside the source tree. Check the coverage limitations and the reference status before using a graph to make a change. An exact method declaration can use virtual dispatch. Only a proven execution target can contribute a recursion point.

## Metrics and structure

Methods with bodies, constructors, accessors, local functions, lambdas, and top-level statements have physical ranges. A switch contributes to cyclomatic complexity according to its alternatives. Boolean chains and nesting contribute to cognitive complexity. XML documentation and other comments do not add code lines. Types use their namespace, containing type, generic arity, and project. Partial declarations keep the physical evidence for each part. Overloads retain their parameter signatures.

The C# parser and tag queries come from the upstream Tree-sitter grammar. The pinned revision includes the C# 15 grammar. Script reports retain the byte ranges of `#load` and `#r`. A relative load can resolve to another discovered script. Assembly references remain external. Razor reports combine template syntax with embedded C#, HTML, JavaScript, and CSS. Components and their code-behind share one partial type when their namespace, name, and generic arity match. The Razor SDK project items control component and view membership. [C# grammar](https://github.com/tree-sitter/tree-sitter-c-sharp/tree/f802dfbeedc4a6102cff8a71b43e1b2453795ccf) and [Razor grammar](https://github.com/alfpricot/tree-sitter-razor-modern/tree/v1.0.0)

## Check project membership against MSBuild

To check a .NET codebase, set `CSHARP_CORPUS` to its absolute path and `DOTNET` to the SDK executable. The following test compares discovered source membership and project references for every Debug project target with MSBuild's evaluated items. The SDK is needed only for this comparison.

```sh
CSHARP_CORPUS=/path/to/repo DOTNET=/path/to/dotnet \
  cargo test --locked --bin entanglement compile_membership_matches_msbuild -- --ignored --nocapture
```
