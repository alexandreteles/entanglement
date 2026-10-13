//! Syntax and block evidence shared by the static build extractors.

use super::DependencyScope;
use super::lexer::{
    Token, TokenKind, ident, ident_at, lex, literal_path, literal_string, matching, matching_open,
    statement_start, symbol_at, token_identifier,
};
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
            evidence.record(Some(node.kind()), node.start_byte(), node.end_byte());
            let mut cursor = node.walk();
            pending.extend(node.children(&mut cursor));
        }
        evidence
    }

    fn record(&mut self, kind: Option<&str>, start: usize, end: usize) {
        let range = (start, end);
        match kind {
            Some("simple_identifier") => {
                self.identifiers.insert(range);
            }
            Some("string_literal") => {
                self.strings.insert(range);
            }
            Some(
                "if_expression" | "when_expression" | "for_statement" | "while_statement"
                | "do_while_statement" | "try_expression",
            ) => self.control_ranges.push(range),
            _ => {}
        }
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

pub(super) fn classify_block(
    tokens: &[Token],
    open: usize,
    syntax: &KotlinSyntax,
    ancestors: &[GradleBlock],
) -> GradleBlock {
    match block_kind_before(tokens, open) {
        Some("kotlin") => return GradleBlock::Kotlin,
        Some("android") => return GradleBlock::Android,
        Some("sourceSets") => return GradleBlock::SourceSets,
        Some("buildTypes") => return GradleBlock::BuildTypes,
        Some("productFlavors") => return GradleBlock::ProductFlavors,
        _ => {}
    }

    match ancestors.last() {
        Some(GradleBlock::SourceSets) => source_set_block_name(tokens, open, syntax)
            .map(GradleBlock::SourceSet)
            .unwrap_or(GradleBlock::Other),
        Some(GradleBlock::BuildTypes | GradleBlock::ProductFlavors) => {
            static_named_block(tokens, open, syntax)
                .map(GradleBlock::Variant)
                .unwrap_or(GradleBlock::Other)
        }
        _ => GradleBlock::Other,
    }
}

fn block_kind_before(tokens: &[Token], open: usize) -> Option<&str> {
    let previous = open.checked_sub(1)?;
    token_identifier(tokens, previous).or_else(|| call_name_before_brace(tokens, open))
}

fn source_set_block_name(tokens: &[Token], open: usize, syntax: &KotlinSyntax) -> Option<String> {
    let previous = open.checked_sub(1)?;
    let name = if matches!(
        token_identifier(tokens, previous),
        Some("getting" | "creating")
    ) {
        let name_index = open.checked_sub(3)?;
        if !ident_at(tokens, open.checked_sub(4)?, "val")
            || !ident_at(tokens, open.checked_sub(2)?, "by")
            || !syntax.permits_identifier(&tokens[name_index])
        {
            return None;
        }
        token_identifier(tokens, name_index)?.to_owned()
    } else {
        static_named_block(tokens, open, syntax)?
    };
    valid_source_set_name(&name).then_some(name)
}

fn static_named_block(tokens: &[Token], open: usize, syntax: &KotlinSyntax) -> Option<String> {
    let close = open.checked_sub(1)?;
    if !symbol_at(tokens, close, ')') {
        let name = token_identifier(tokens, close)?;
        if matches!(name, "getting" | "creating") {
            return None;
        }
        return (syntax.permits_identifier(&tokens[close]) && valid_source_set_name(name))
            .then(|| name.to_owned());
    }

    let paren = matching_open(tokens, close, '(', ')')?;
    let callee = token_identifier(tokens, paren.checked_sub(1)?)?;
    if !matches!(callee, "getByName" | "named" | "create" | "maybeCreate") {
        return None;
    }
    let name = super::lexer::literal_string(tokens, paren + 1, close, |token| {
        syntax.permits_string(token)
    })?;
    valid_source_set_name(&name).then_some(name)
}

fn call_name_before_brace(tokens: &[Token], open: usize) -> Option<&str> {
    let close = open.checked_sub(1)?;
    if !symbol_at(tokens, close, ')') {
        return None;
    }
    let paren = matching_open(tokens, close, '(', ')')?;
    token_identifier(tokens, paren.checked_sub(1)?)
}

pub(super) fn valid_source_set_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
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
