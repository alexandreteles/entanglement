use serde::{Deserialize, Serialize};

/// A snapshot of internal units and source-backed dependencies.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StructureSnapshot {
    pub scope: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub removed_paths: Vec<String>,
    pub units: Vec<Unit>,
    pub relationships: Vec<Relationship>,
    pub coverage: Vec<Coverage>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnitKind {
    Module,
    Type,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Projection {
    Contract,
    Full,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Unit {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structural_fingerprint: Option<String>,
    pub kind: UnitKind,
    pub path: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationshipKind {
    Type,
    Call,
    Field,
    Inherit,
    Implement,
    Compose,
    Import,
    Reexport,
    Resource,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionStatus {
    Resolved,
    External,
    Ambiguous,
    Unresolved,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Relationship {
    pub source: String,
    pub target: String,
    pub kind: RelationshipKind,
    pub contract: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub ordered: bool,
    pub resolution: ResolutionStatus,
    pub evidence: Vec<Evidence>,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub struct Evidence {
    pub path: String,
    pub start: usize,
    pub end: usize,
    pub owner: String,
    pub detail: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoverageStatus {
    Complete,
    Partial,
    Unavailable,
    Inapplicable,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Coverage {
    pub path: String,
    pub language: String,
    pub status: CoverageStatus,
    pub effective: CoverageStatus,
    pub limitations: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GraphAnalysis {
    pub granularity: UnitKind,
    pub projection: Projection,
    pub units: Vec<String>,
    pub edges: Vec<Relationship>,
    pub boundary: Vec<Relationship>,
    pub cycles: Vec<Cycle>,
    pub pc: PropagationCost,
    pub impacts: Vec<Impact>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PropagationCost {
    pub value: Option<f64>,
    pub numerator: u64,
    pub denominator: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Witness {
    pub units: Vec<String>,
    pub evidence: Vec<Evidence>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Cycle {
    pub members: Vec<String>,
    pub witness: Witness,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Impact {
    pub unit: usize,
    pub dependencies: Vec<usize>,
    pub dependents: Vec<usize>,
    pub predecessors: Vec<[usize; 2]>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StructuralDelta {
    pub events: Vec<StructuralEvent>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StructuralEvent {
    pub kind: String,
    pub granularity: UnitKind,
    pub projection: Projection,
    pub units: Vec<String>,
    pub before: Vec<String>,
    pub after: Vec<String>,
    pub changed: Vec<Relationship>,
    pub before_path: Option<Witness>,
    pub after_path: Option<Witness>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub before_paths: Vec<Witness>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub after_paths: Vec<Witness>,
}
