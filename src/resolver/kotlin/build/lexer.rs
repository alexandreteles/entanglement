mod scanner;
pub(super) use scanner::lex;

#[derive(Clone, Debug)]
pub(super) struct Token {
    pub(super) kind: TokenKind,
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) line: usize,
}

#[derive(Clone, Debug)]
pub(super) enum TokenKind {
    Identifier(String),
    String(Option<String>),
    Symbol(char),
}

pub(super) fn ident(token: &Token, expected: &str) -> bool {
    matches!(&token.kind, TokenKind::Identifier(name) if name == expected)
}

pub(super) fn ident_at(tokens: &[Token], index: usize, expected: &str) -> bool {
    tokens
        .get(index)
        .is_some_and(|token| ident(token, expected))
}

pub(super) fn token_identifier(tokens: &[Token], index: usize) -> Option<&str> {
    match &tokens.get(index)?.kind {
        TokenKind::Identifier(name) => Some(name),
        _ => None,
    }
}

fn symbol(token: &Token, expected: char) -> bool {
    matches!(token.kind, TokenKind::Symbol(actual) if actual == expected)
}

pub(super) fn symbol_at(tokens: &[Token], index: usize, expected: char) -> bool {
    tokens
        .get(index)
        .is_some_and(|token| symbol(token, expected))
}

pub(super) fn literal_string(
    tokens: &[Token],
    start: usize,
    end: usize,
    permits_string: impl FnOnce(&Token) -> bool,
) -> Option<String> {
    let token = tokens.get(start)?;
    if end != start + 1 || !permits_string(token) {
        return None;
    }
    match &token.kind {
        TokenKind::String(Some(value)) => Some(value.clone()),
        _ => None,
    }
}

pub(super) fn literal_path(
    tokens: &[Token],
    start: usize,
    end: usize,
    permits_string: impl FnOnce(&Token) -> bool,
) -> Option<String> {
    let path = literal_string(tokens, start, end, permits_string)?;
    valid_gradle_path(&path).then_some(path)
}

fn valid_gradle_path(path: &str) -> bool {
    path.starts_with(':')
        && path.strip_prefix(':').is_some_and(|rest| {
            !rest.is_empty()
                && rest.split(':').all(|part| {
                    !part.is_empty()
                        && part.chars().all(|character| {
                            character.is_ascii_alphanumeric()
                                || matches!(character, '_' | '-' | '.')
                        })
                })
        })
}

pub(super) fn matching(tokens: &[Token], open: usize, left: char, right: char) -> Option<usize> {
    symbol_at(tokens, open, left).then_some(())?;
    scan_matching(tokens.iter().enumerate().skip(open), left, right)
}

pub(super) fn matching_open(
    tokens: &[Token],
    close: usize,
    left: char,
    right: char,
) -> Option<usize> {
    symbol_at(tokens, close, right).then_some(())?;
    scan_matching(tokens.iter().enumerate().take(close + 1).rev(), right, left)
}

fn scan_matching<'token>(
    tokens: impl Iterator<Item = (usize, &'token Token)>,
    opening: char,
    closing: char,
) -> Option<usize> {
    let mut depth = 0usize;
    for (index, token) in tokens {
        if symbol(token, opening) {
            depth += 1;
        } else if symbol(token, closing) {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(index);
            }
        }
    }
    None
}

pub(super) fn statement_start(tokens: &[Token], index: usize) -> bool {
    index == 0
        || symbol_at(tokens, index - 1, '{')
        || symbol_at(tokens, index - 1, '}')
        || symbol_at(tokens, index - 1, ';')
        || tokens[index - 1].line < tokens[index].line
}

pub(super) fn statement_end(tokens: &[Token], start: usize) -> usize {
    let line = tokens.get(start).map_or(usize::MAX, |token| token.line);
    let mut nesting = Nesting::default();
    let mut index = start;
    while let Some(token) = tokens.get(index) {
        if token.line > line && nesting.is_statement_boundary() {
            break;
        }
        match token.kind {
            TokenKind::Symbol(';') | TokenKind::Symbol('}') if nesting.is_statement_boundary() => {
                break;
            }
            TokenKind::Symbol(symbol) => nesting.advance(symbol),
            _ => {}
        }
        index += 1;
    }
    index
}

pub(super) fn split_arguments(tokens: &[Token], start: usize, end: usize) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut argument_start = start;
    let mut nesting = Nesting::default();
    for (index, token) in tokens.iter().enumerate().take(end).skip(start) {
        match token.kind {
            TokenKind::Symbol(',') if nesting.is_empty() => {
                push_range(&mut ranges, argument_start, index);
                argument_start = index + 1;
            }
            TokenKind::Symbol(symbol) => nesting.advance(symbol),
            _ => {}
        }
    }
    push_range(&mut ranges, argument_start, end);
    ranges
}

fn push_range(ranges: &mut Vec<(usize, usize)>, start: usize, end: usize) {
    if start < end {
        ranges.push((start, end));
    }
}

#[derive(Default)]
struct Nesting {
    parens: usize,
    braces: usize,
    brackets: usize,
}

impl Nesting {
    fn advance(&mut self, symbol: char) {
        match symbol {
            '(' => self.parens += 1,
            ')' => self.parens = self.parens.saturating_sub(1),
            '{' => self.braces += 1,
            '}' => self.braces = self.braces.saturating_sub(1),
            '[' => self.brackets += 1,
            ']' => self.brackets = self.brackets.saturating_sub(1),
            _ => {}
        }
    }

    fn is_empty(&self) -> bool {
        self.parens == 0 && self.braces == 0 && self.brackets == 0
    }

    fn is_statement_boundary(&self) -> bool {
        self.parens == 0 && self.brackets == 0
    }
}
