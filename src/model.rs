use std::path::PathBuf;

use serde::Serialize;

use crate::metrics::halstead::{HalsteadMetrics, HalsteadTokenKind};
use crate::metrics::maintainability::{MaintainabilityBand, MaintainabilityIndex};

#[derive(Debug, Clone, Serialize)]
/// The full report for one command.
pub struct AnalysisResult {
    /// The reports for supported source files.
    pub files: Vec<FileAnalysis>,
    /// The exact Microsoft Maintainability Index ranges and colors.
    pub maintainability_index_bands: [MaintainabilityBand; 3],
    /// The patch report, when the command analyzes a patch.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub patch: Option<PatchAnalysis>,
}

#[derive(Debug, Clone, Serialize)]
/// The analysis report for one source file.
pub struct FileAnalysis {
    /// The displayed file path.
    pub path: String,
    /// The BLAKE3 hash of the source bytes.
    pub hash: String,
    /// The language name from grammar metadata.
    pub language: String,
    /// The number of source rows with code.
    pub nloc: usize,
    /// Halstead operators, operands, and derived metrics for the whole file.
    pub halstead: HalsteadMetrics,
    /// Microsoft Maintainability Index calculated from volume, file CC, and NLOC.
    pub maintainability_index: Option<MaintainabilityIndex>,
    /// The metrics for functions in this file.
    pub functions: Vec<FunctionAnalysis>,
    /// The source ranges that use an injected language.
    pub injections: Vec<InjectionAnalysis>,
    /// The resolution result for each captured reference.
    pub resolution: Vec<ReferenceAnalysis>,
}

#[derive(Debug, Clone, Serialize)]
/// The metrics and complexity changes for one function.
pub struct FunctionAnalysis {
    /// The function name from source.
    pub name: String,
    /// The inclusive start byte of the function item.
    pub start_byte: usize,
    /// The exclusive end byte of the function item.
    pub end_byte: usize,
    /// The number of source rows with code in the function.
    pub nloc: usize,
    /// Halstead operators, operands, and derived metrics for the function.
    pub halstead: HalsteadMetrics,
    /// Microsoft Maintainability Index calculated from volume, function CC, and NLOC.
    pub maintainability_index: Option<MaintainabilityIndex>,
    /// The cyclomatic complexity score.
    pub cyclomatic_complexity: usize,
    /// The complexity score divided by NLOC, with zero treated as one.
    pub cyclomatic_density: f64,
    /// The source events that contribute to complexity.
    pub contributions: Vec<ComplexityContribution>,
    /// The SonarSource cognitive complexity score.
    pub cognitive_complexity: usize,
    /// The source events that contribute to cognitive complexity.
    pub cognitive_contributions: Vec<ComplexityContribution>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
/// One source event that adds to cyclomatic complexity.
pub struct ComplexityContribution {
    /// The event kind, such as `condition` or `multiway`.
    pub kind: String,
    /// The complexity added by this event.
    pub value: i32,
    /// The inclusive start byte of the event.
    pub start_byte: usize,
    /// The exclusive end byte of the event.
    pub end_byte: usize,
    /// The one-based source line of the event.
    pub line: usize,
}

#[derive(Debug, Clone, Serialize)]
/// One injected-language source range.
pub struct InjectionAnalysis {
    /// The injected language name from grammar metadata.
    pub language: String,
    /// The inclusive start byte of the range.
    pub start_byte: usize,
    /// The exclusive end byte of the range.
    pub end_byte: usize,
    /// True when an analyzer supports and processes this range.
    pub analyzed: bool,
}

#[derive(Debug, Clone, Serialize)]
/// The resolution for one source reference.
pub struct ReferenceAnalysis {
    /// The path segments captured from source.
    pub path: Vec<String>,
    /// The inclusive start byte of the reference.
    pub start_byte: usize,
    /// The exclusive end byte of the reference.
    pub end_byte: usize,
    /// The matching result.
    pub resolution: Resolution,
}

#[derive(Debug, Clone, Serialize)]
/// A reference result: exact, ambiguous, external, or unresolved.
#[serde(tag = "status", content = "symbols", rename_all = "snake_case")]
pub enum Resolution {
    /// One local symbol matches the reference.
    Exact(SymbolId),
    /// More than one local symbol matches the reference.
    Ambiguous(Vec<SymbolId>),
    /// The reference names a symbol outside the analyzed files.
    External,
    /// The analyzer cannot find a valid target.
    Unresolved,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
/// A stable identity for one local definition.
pub struct SymbolId {
    /// The file that contains the definition.
    pub file: String,
    /// The inline module path inside that file.
    pub module: ModulePath,
    /// The definition name.
    pub name: String,
    /// The definition kind.
    pub kind: DefinitionKind,
    /// The definition's start byte, used to distinguish same-name items.
    pub start_byte: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
/// The inline module path inside one source file.
pub struct ModulePath(
    /// The module names, from the outer module to the inner module.
    pub Vec<String>,
);

#[derive(Debug, Clone, Serialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
/// The kind of a local definition.
#[serde(rename_all = "snake_case")]
pub enum DefinitionKind {
    /// A module declaration.
    Module,
    /// A free function.
    Function,
    /// A struct.
    Struct,
    /// An enum.
    Enum,
    /// A union.
    Union,
    /// A trait.
    Trait,
    /// A type alias.
    TypeAlias,
    /// A constant item.
    Constant,
    /// A static item.
    Static,
    /// A macro definition.
    Macro,
    /// A method in an `impl` or trait.
    Method,
    /// A definition that has no more specific supported kind.
    Other,
}

#[derive(Debug, Clone, Serialize)]
/// The reports for the files touched by a patch.
pub struct PatchAnalysis {
    /// The before and after reports for each touched path.
    pub files: Vec<FilePatchAnalysis>,
}

#[derive(Debug, Clone, Serialize)]
/// The analysis result for one changed path.
pub struct FilePatchAnalysis {
    /// The path in the patch.
    pub path: String,
    /// The file report before the patch, or no value for a new file.
    pub before: Option<FileAnalysis>,
    /// The file report after the patch, or no value for a deleted file.
    pub after: Option<FileAnalysis>,
    /// The Halstead metric and token contribution changes for this file.
    pub halstead: HalsteadDelta,
    /// The file's before/after Microsoft Maintainability Index and score effects.
    pub maintainability_index: MaintainabilityDelta,
    /// The metric and contribution changes for matching functions.
    pub functions: Vec<FunctionDelta>,
}

#[derive(Debug, Clone, Serialize)]
/// The metric and contribution changes for one function.
pub struct FunctionDelta {
    /// The function name.
    pub name: String,
    /// The inclusive start and exclusive end source byte offsets before the patch.
    /// JSON uses null when this side has no matching function.
    pub before_range: Option<SourceRange>,
    /// The inclusive start and exclusive end source byte offsets after the patch.
    /// JSON uses null when this side has no matching function.
    pub after_range: Option<SourceRange>,
    /// The NLOC values before and after the patch.
    pub nloc: MetricDelta<usize>,
    /// The cyclomatic complexity values before and after the patch.
    pub cyclomatic_complexity: MetricDelta<usize>,
    /// The cyclomatic density values before and after the patch.
    pub cyclomatic_density: MetricDelta<f64, f64>,
    /// The cognitive complexity values before and after the patch.
    pub cognitive_complexity: MetricDelta<usize>,
    /// The Halstead metric and token contribution changes for this function.
    pub halstead: HalsteadDelta,
    /// The function's before/after Microsoft Maintainability Index and score effects.
    pub maintainability_index: MaintainabilityDelta,
    /// The complexity events added by the patch.
    pub added_contributions: Vec<ComplexityContribution>,
    /// The complexity events removed by the patch.
    pub removed_contributions: Vec<ComplexityContribution>,
    /// The cognitive complexity events added by the patch.
    pub added_cognitive_contributions: Vec<ComplexityContribution>,
    /// The cognitive complexity events removed by the patch.
    pub removed_cognitive_contributions: Vec<ComplexityContribution>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
/// A source range expressed as byte offsets.
pub struct SourceRange {
    /// The inclusive start byte of the range.
    pub start_byte: usize,
    /// The exclusive end byte of the range.
    pub end_byte: usize,
}

#[derive(Debug, Clone, Serialize)]
/// The Halstead indicators and lexical contributors changed by a patch.
pub struct HalsteadDelta {
    /// The distinct operator count (n1) before and after the patch.
    pub distinct_operators: MetricDelta<usize>,
    /// The distinct operand count (n2) before and after the patch.
    pub distinct_operands: MetricDelta<usize>,
    /// The operator occurrence count (N1) before and after the patch.
    pub total_operators: MetricDelta<usize>,
    /// The operand occurrence count (N2) before and after the patch.
    pub total_operands: MetricDelta<usize>,
    /// The vocabulary before and after the patch.
    pub vocabulary: MetricDelta<usize>,
    /// The source length before and after the patch.
    pub length: MetricDelta<usize>,
    /// The estimated length before and after the patch.
    pub estimated_length: MetricDelta<f64, f64>,
    /// The volume before and after the patch.
    pub volume: MetricDelta<f64, f64>,
    /// The difficulty before and after the patch.
    pub difficulty: MetricDelta<f64, f64>,
    /// The effort before and after the patch.
    pub effort: MetricDelta<f64, f64>,
    /// The estimated time before and after the patch.
    pub time: MetricDelta<f64, f64>,
    /// The program level before and after the patch.
    pub program_level: MetricDelta<f64, f64>,
    /// The estimated bugs before and after the patch.
    pub estimated_bugs: MetricDelta<f64, f64>,
    /// Lexical contributors added by the patch; counts are positive.
    pub added_tokens: Vec<HalsteadTokenChange>,
    /// Lexical contributors removed by the patch; counts are positive.
    pub removed_tokens: Vec<HalsteadTokenChange>,
}

#[derive(Debug, Clone, Serialize)]
/// A grouped Halstead token occurrence change.
pub struct HalsteadTokenChange {
    /// Whether these occurrences are operators or operands.
    pub kind: HalsteadTokenKind,
    /// The exact token spelling.
    pub token: String,
    /// A representative source line for this group.
    pub line: usize,
    /// The number of occurrences added or removed.
    pub count: usize,
}

#[derive(Debug, Clone, Serialize)]
/// The before/after Microsoft Maintainability Index for a patch scope.
pub struct MaintainabilityDelta {
    /// The measured before value, absent when the scope did not exist.
    pub before: Option<MaintainabilityIndex>,
    /// The measured after value, absent when the scope did not exist.
    pub after: Option<MaintainabilityIndex>,
    /// The score change when both sides have a measured index; positive improves.
    pub score: Option<MetricDelta<f64, f64>>,
    /// The point effect caused by a Halstead volume change.
    pub volume_effect: Option<f64>,
    /// The point effect caused by a cyclomatic complexity change.
    pub cyclomatic_effect: Option<f64>,
    /// The point effect caused by an NLOC change.
    pub nloc_effect: Option<f64>,
    /// The point adjustment caused by clamping to the score range.
    pub clamp_adjustment: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
/// One metric value before and after a patch, with its change.
pub struct MetricDelta<T, D = i64> {
    /// The metric before the patch.
    pub before: T,
    /// The metric after the patch.
    pub after: T,
    /// The change from `before` to `after`.
    pub delta: D,
}

#[derive(Debug, Clone)]
pub(crate) struct FileFacts {
    pub source: std::sync::Arc<[u8]>,
    /// The resolved identity of the source file.
    pub target: PathBuf,
    /// All logical paths that name this source file.
    pub aliases: Vec<PathBuf>,
    /// The selected logical path for the report.
    pub path: PathBuf,
    pub module: ModulePath,
    pub definitions: Vec<Definition>,
    pub imports: Vec<Import>,
    pub references: Vec<Reference>,
    pub locals: Vec<LocalBinding>,
    pub analysis: FileAnalysis,
}

#[derive(Debug, Clone)]
pub(crate) struct LocalBinding {
    pub name: String,
    pub start_byte: usize,
    pub end_byte: usize,
    pub scope_start: usize,
    pub scope_end: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct Definition {
    pub name: String,
    pub kind: DefinitionKind,
    pub module: ModulePath,
    pub start_byte: usize,
    pub end_byte: usize,
    pub scope_start: usize,
    pub scope_end: usize,
    pub is_public: bool,
    pub inline_module: bool,
    pub external_module: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct Import {
    pub path: Vec<String>,
    pub alias: Option<String>,
    pub start_byte: usize,
    pub end_byte: usize,
    pub module: ModulePath,
    pub scope_start: usize,
    pub scope_end: usize,
    pub is_public: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct Reference {
    pub path: Vec<String>,
    pub module: ModulePath,
    pub kind: ReferenceKind,
    pub start_byte: usize,
    pub end_byte: usize,
    pub scope_start: usize,
    pub scope_end: usize,
    /// Start byte of the enclosing function when this reference is a call
    /// target outside a deferred closure or async block.
    pub call_owner: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ReferenceKind {
    Call,
    Qualified,
    Method,
    Type,
    Value,
}
