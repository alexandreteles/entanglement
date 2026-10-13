use std::path::PathBuf;

use super::lexer::{
    Token, ident, lex, literal_path, literal_string, matching, statement_start, symbol_at,
};
use super::model::GradleBlock;
use super::scopes::GradleScopes;
use super::syntax::KotlinSyntax;

/// One immutable view of a Gradle script shared by all static extractors.
/// Token positions, syntax evidence, brace ancestry, and control-flow evidence
/// are built once instead of being rescanned independently by each parser.
pub(super) struct StaticScript {
    tokens: Vec<Token>,
    syntax: KotlinSyntax,
    scopes: GradleScopes,
}

impl StaticScript {
    pub(super) fn parse(source: &str) -> Self {
        let tokens = lex(source);
        let syntax = KotlinSyntax::parse(source);
        let scopes = GradleScopes::scan(&tokens, &syntax);
        Self {
            tokens,
            syntax,
            scopes,
        }
    }

    pub(super) fn tokens(&self) -> &[Token] {
        &self.tokens
    }

    pub(super) fn syntax(&self) -> &KotlinSyntax {
        &self.syntax
    }

    pub(super) fn scopes(&self) -> &GradleScopes {
        &self.scopes
    }

    pub(super) fn ancestors(&self, index: usize) -> &[GradleBlock] {
        self.scopes.at(index)
    }

    pub(super) fn brace_depth(&self, index: usize) -> Option<usize> {
        self.scopes.brace_depth(index)
    }

    pub(super) fn is_identifier(&self, index: usize, name: &str) -> bool {
        self.tokens
            .get(index)
            .is_some_and(|token| ident(token, name))
    }

    pub(super) fn is_static_identifier(&self, index: usize, name: &str) -> bool {
        self.is_static_token(index)
            && self
                .tokens
                .get(index)
                .is_some_and(|token| ident(token, name))
    }

    pub(super) fn is_static_token(&self, index: usize) -> bool {
        self.tokens.get(index).is_some_and(|token| {
            self.syntax.permits_identifier(token) && self.syntax.outside_control_flow(token)
        })
    }

    pub(super) fn outside_control_flow(&self, index: usize) -> bool {
        self.tokens
            .get(index)
            .is_some_and(|token| self.syntax.outside_control_flow(token))
    }

    pub(super) fn is_root_statement(&self, index: usize, name: &str) -> bool {
        self.is_static_identifier(index, name)
            && self.ancestors(index).is_empty()
            && statement_start(&self.tokens, index)
    }

    pub(super) fn is_symbol(&self, index: usize, expected: char) -> bool {
        symbol_at(&self.tokens, index, expected)
    }

    pub(super) fn braced_call_at(&self, index: usize) -> Option<(usize, usize)> {
        let mut open = index + 1;
        if self.is_symbol(open, '(') {
            open = matching(&self.tokens, open, '(', ')')? + 1;
        }
        self.is_symbol(open, '{').then_some(())?;
        Some((open, matching(&self.tokens, open, '{', '}')?))
    }

    pub(super) fn literal_string(&self, start: usize, end: usize) -> Option<String> {
        literal_string(&self.tokens, start, end, |token| {
            self.syntax.permits_string(token)
        })
    }

    pub(super) fn literal_path(&self, start: usize, end: usize) -> Option<String> {
        literal_path(&self.tokens, start, end, |token| {
            self.syntax.permits_string(token)
        })
    }

    pub(super) fn project_directory(&self, index: usize) -> Option<(String, PathBuf)> {
        let project_close = matching(&self.tokens, index + 1, '(', ')')?;
        let module_path = self.literal_path(index + 2, project_close)?;
        let assignment = project_close + 1;
        if !(self.is_symbol(assignment, '.')
            && self.is_identifier(assignment + 1, "projectDir")
            && self.is_symbol(assignment + 2, '='))
        {
            return None;
        }

        let file_call = assignment + 3;
        if !self.is_identifier(file_call, "file") || !self.is_symbol(file_call + 1, '(') {
            return None;
        }
        let file_close = matching(&self.tokens, file_call + 1, '(', ')')?;
        let directory = PathBuf::from(self.literal_string(file_call + 2, file_close)?);
        (!directory.is_absolute()).then_some((module_path, directory))
    }
}
