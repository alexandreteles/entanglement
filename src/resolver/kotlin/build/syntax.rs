//! Syntax and block evidence shared by the static build extractors.

use super::DependencyScope;

mod scopes;
use super::lexer::{
    Token, TokenKind, ident_at, lex, literal_path, literal_string, matching, matching_open,
    statement_start, symbol_at, token_identifier,
};
pub(super) use scopes::*;
use std::collections::HashSet;
use std::path::PathBuf;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum GradleBlock {
    Kotlin,
    Android,
    SourceSets,
    BuildTypes,
    ProductFlavors,
    SourceSet(String),
    Variant(String),
    Other,
}

#[derive(Default)]
pub(super) struct KotlinSyntax {
    valid: bool,
    identifiers: HashSet<(usize, usize)>,
    strings: HashSet<(usize, usize)>,
    control_ranges: Vec<(usize, usize)>,
}

impl KotlinSyntax {
    pub(super) fn parse(source: &str) -> Self {
        let mut parser = tree_sitter::Parser::new();
        let language: tree_sitter::Language = tree_sitter_kotlin::LANGUAGE.into();
        if parser.set_language(&language).is_err() {
            return Self::default();
        }
        let Some(tree) = parser.parse(source, None) else {
            return Self::default();
        };

        let root = tree.root_node();
        let mut evidence = Self {
            valid: !root.has_error(),
            ..Self::default()
        };
        let mut pending = vec![root];
        while let Some(node) = pending.pop() {
            let range = (node.start_byte(), node.end_byte());
            match node.kind() {
                "simple_identifier" => {
                    evidence.identifiers.insert(range);
                }
                "string_literal" => {
                    evidence.strings.insert(range);
                }
                "if_expression" | "when_expression" | "for_statement" | "while_statement"
                | "do_while_statement" | "try_expression" => evidence.control_ranges.push(range),
                _ => {}
            }
            let mut cursor = node.walk();
            pending.extend(node.children(&mut cursor));
        }
        evidence
    }

    pub(super) fn permits_identifier(&self, token: &Token) -> bool {
        !self.valid || self.identifiers.contains(&(token.start, token.end))
    }

    pub(super) fn permits_string(&self, token: &Token) -> bool {
        !self.valid || self.strings.contains(&(token.start, token.end))
    }

    pub(super) fn outside_control_flow(&self, token: &Token) -> bool {
        !self
            .control_ranges
            .iter()
            .any(|(start, end)| *start <= token.start && token.end <= *end)
    }
}

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
        ident_at(&self.tokens, index, name)
    }

    pub(super) fn is_static_identifier(&self, index: usize, name: &str) -> bool {
        self.is_static_token(index) && self.is_identifier(index, name)
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
