use std::collections::BTreeSet;

use serde::Serialize;

use super::cyclomatic::FunctionScope;

/// The two lexical classes used by Halstead's source metrics.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum HalsteadTokenKind {
    /// A language keyword, punctuation token, or operator symbol.
    Operator,
    /// An identifier, literal, or lifetime/label token.
    Operand,
}

/// One source occurrence of a Halstead operator or operand.
#[derive(Debug, Clone, Serialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HalsteadToken {
    /// The lexical class of this occurrence.
    pub kind: HalsteadTokenKind,
    /// The exact source spelling, serialized as `text` in reports.
    #[serde(rename = "text")]
    pub token: String,
    /// The inclusive source byte offset.
    pub start_byte: usize,
    /// The exclusive source byte offset.
    pub end_byte: usize,
    /// The one-based source line.
    pub line: usize,
}

/// Halstead's standard metrics for a token set.
///
/// Operators are counted by exact spelling, as are operands. Rust's language
/// adapter defines which syntax tokens belong to each class. The calculations
/// themselves only use the four standard counts, so this type can also be
/// created directly from counts without parsing source.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct HalsteadMetrics {
    /// Number of distinct operators (n1).
    pub distinct_operators: usize,
    /// Number of distinct operands (n2).
    pub distinct_operands: usize,
    /// Total operator occurrences (N1).
    pub total_operators: usize,
    /// Total operand occurrences (N2).
    pub total_operands: usize,
    /// Program vocabulary, n1 + n2.
    pub vocabulary: usize,
    /// Program length, N1 + N2.
    pub length: usize,
    /// Estimated length, n1 log2(n1) + n2 log2(n2).
    pub estimated_length: f64,
    /// Program volume, N log2(n).
    pub volume: f64,
    /// Program difficulty, (n1 / 2) * (N2 / n2).
    pub difficulty: f64,
    /// Program effort, D * V.
    pub effort: f64,
    /// Estimated time in seconds, E / 18.
    pub time: f64,
    /// Program level, 1 / D.
    pub program_level: f64,
    /// Estimated delivered bugs, V / 3000.
    pub estimated_bugs: f64,
    /// Operator occurrences in source order.
    pub operators: Vec<HalsteadToken>,
    /// Operand occurrences in source order.
    pub operands: Vec<HalsteadToken>,
}

impl HalsteadMetrics {
    /// Calculate Halstead metrics from the four standard token counts.
    ///
    /// Empty vocabularies have zero estimated length, volume, difficulty,
    /// effort, time, program level, and estimated bugs. This keeps all reports
    /// finite; callers can distinguish empty input from nonempty input by the
    /// counts and token lists.
    pub fn calculate(
        distinct_operators: usize,
        distinct_operands: usize,
        total_operators: usize,
        total_operands: usize,
    ) -> Self {
        let vocabulary = distinct_operators.saturating_add(distinct_operands);
        let length = total_operators.saturating_add(total_operands);
        let estimated_length = log_term(distinct_operators) + log_term(distinct_operands);
        let volume = if vocabulary == 0 || length == 0 {
            0.0
        } else {
            length as f64 * (vocabulary as f64).log2()
        };
        let difficulty = if distinct_operands == 0 {
            0.0
        } else {
            (distinct_operators as f64 / 2.0) * (total_operands as f64 / distinct_operands as f64)
        };
        let effort = difficulty * volume;

        Self {
            distinct_operators,
            distinct_operands,
            total_operators,
            total_operands,
            vocabulary,
            length,
            estimated_length,
            volume,
            difficulty,
            effort,
            time: effort / 18.0,
            program_level: if difficulty == 0.0 {
                0.0
            } else {
                1.0 / difficulty
            },
            estimated_bugs: volume / 3000.0,
            operators: Vec::new(),
            operands: Vec::new(),
        }
    }

    /// Calculate metrics from token occurrences and retain them for reports.
    pub fn from_tokens(tokens: impl IntoIterator<Item = HalsteadToken>) -> Self {
        let mut operators = Vec::new();
        let mut operands = Vec::new();
        let mut distinct_operators = BTreeSet::new();
        let mut distinct_operands = BTreeSet::new();

        for token in tokens {
            match token.kind {
                HalsteadTokenKind::Operator => {
                    distinct_operators.insert(token.token.clone());
                    operators.push(token);
                }
                HalsteadTokenKind::Operand => {
                    distinct_operands.insert(token.token.clone());
                    operands.push(token);
                }
            }
        }
        operators.sort_by_key(|token| (token.start_byte, token.end_byte));
        operands.sort_by_key(|token| (token.start_byte, token.end_byte));
        let mut result = Self::calculate(
            distinct_operators.len(),
            distinct_operands.len(),
            operators.len(),
            operands.len(),
        );
        result.operators = operators;
        result.operands = operands;
        result
    }

    /// Return all occurrences in deterministic source order.
    pub fn tokens(&self) -> Vec<&HalsteadToken> {
        let mut tokens = self
            .operators
            .iter()
            .chain(&self.operands)
            .collect::<Vec<_>>();
        tokens.sort_by_key(|token| (token.start_byte, token.end_byte));
        tokens
    }
}

/// Assign each occurrence to its smallest containing function scope.
pub fn assign_tokens(
    functions: &[FunctionScope],
    tokens: &[HalsteadToken],
) -> std::collections::BTreeMap<usize, Vec<HalsteadToken>> {
    let mut assigned = std::collections::BTreeMap::<usize, Vec<HalsteadToken>>::new();
    for token in tokens {
        if let Some((index, _)) = functions
            .iter()
            .enumerate()
            .filter(|(_, function)| {
                function.range.start <= token.start_byte && token.end_byte <= function.range.end
            })
            .min_by_key(|(_, function)| function.range.end - function.range.start)
        {
            assigned.entry(index).or_default().push(token.clone());
        }
    }
    assigned
}

fn log_term(value: usize) -> f64 {
    if value == 0 {
        0.0
    } else {
        value as f64 * (value as f64).log2()
    }
}

#[cfg(test)]
#[path = "../../tests/unit/halstead.rs"]
mod tests;
