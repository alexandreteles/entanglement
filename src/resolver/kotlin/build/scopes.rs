use super::blocks::classify_block;
use super::lexer::{Token, TokenKind, symbol_at, token_identifier};
use super::model::{DependencyScope, GradleBlock};
use super::syntax::KotlinSyntax;

pub(super) struct GradleScopes {
    ancestors: Vec<Vec<GradleBlock>>,
    brace_depths: Vec<usize>,
}

impl GradleScopes {
    pub(super) fn scan(tokens: &[Token], syntax: &KotlinSyntax) -> Self {
        let mut stack = Vec::new();
        let mut ancestors = Vec::with_capacity(tokens.len());
        let mut brace_depths = Vec::with_capacity(tokens.len());
        for (index, token) in tokens.iter().enumerate() {
            if matches!(token.kind, TokenKind::Symbol('}')) {
                stack.pop();
            }
            ancestors.push(stack.clone());
            brace_depths.push(stack.len());
            if matches!(token.kind, TokenKind::Symbol('{')) {
                stack.push(classify_block(tokens, index, syntax, &stack));
            }
        }
        Self {
            ancestors,
            brace_depths,
        }
    }

    pub(super) fn at(&self, index: usize) -> &[GradleBlock] {
        self.ancestors.get(index).map_or(&[], Vec::as_slice)
    }

    pub(super) fn brace_depth(&self, index: usize) -> Option<usize> {
        self.brace_depths.get(index).copied()
    }
}

pub(super) fn dependency_scope(
    tokens: &[Token],
    syntax: &KotlinSyntax,
    scopes: &GradleScopes,
    index: usize,
) -> Option<DependencyScope> {
    let ancestry = scopes.at(index);
    match ancestry {
        [] => Some(DependencyScope::Production),
        [
            GradleBlock::Kotlin,
            GradleBlock::SourceSets,
            GradleBlock::SourceSet(name),
        ]
        | [
            GradleBlock::Android,
            GradleBlock::SourceSets,
            GradleBlock::SourceSet(name),
        ] => Some(DependencyScope::Variant(name.clone())),
        [
            GradleBlock::Android,
            GradleBlock::BuildTypes,
            GradleBlock::Variant(name),
        ]
        | [
            GradleBlock::Android,
            GradleBlock::ProductFlavors,
            GradleBlock::Variant(name),
        ] => Some(DependencyScope::Variant(name.clone())),
        [GradleBlock::Kotlin, GradleBlock::SourceSets]
            if symbol_at(tokens, index.saturating_sub(1), '.') =>
        {
            let name_index = index.saturating_sub(2);
            let name = token_identifier(tokens, name_index)?;
            syntax
                .permits_identifier(&tokens[name_index])
                .then(|| DependencyScope::Variant(name.to_owned()))
        }
        _ => None,
    }
}
