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

pub(crate) fn lex(source: &str) -> Vec<Token> {
    Scanner::new(source).collect()
}

struct Scanner<'a> {
    source: &'a str,
    bytes: &'a [u8],
    offset: usize,
    line: usize,
}

impl<'a> Scanner<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            bytes: source.as_bytes(),
            offset: 0,
            line: 0,
        }
    }

    fn skip_trivia(&mut self) {
        while let Some(byte) = self.bytes.get(self.offset).copied() {
            if byte.is_ascii_whitespace() {
                self.line += usize::from(byte == b'\n');
                self.offset += 1;
            } else if self.bytes.get(self.offset..self.offset + 2) == Some(b"//") {
                self.offset += 2;
                while self
                    .bytes
                    .get(self.offset)
                    .is_some_and(|byte| *byte != b'\n')
                {
                    self.offset += 1;
                }
            } else if self.bytes.get(self.offset..self.offset + 2) == Some(b"/*") {
                self.skip_block_comment();
            } else {
                break;
            }
        }
    }

    fn skip_block_comment(&mut self) {
        self.offset += 2;
        let mut depth = 1usize;
        while self.offset < self.bytes.len() && depth > 0 {
            if self.bytes.get(self.offset..self.offset + 2) == Some(b"/*") {
                depth += 1;
                self.offset += 2;
            } else if self.bytes.get(self.offset..self.offset + 2) == Some(b"*/") {
                depth -= 1;
                self.offset += 2;
            } else {
                self.line += usize::from(self.bytes[self.offset] == b'\n');
                self.offset += 1;
            }
        }
    }

    fn scan_string(&mut self) -> Token {
        let start = self.offset;
        let line = self.line;
        let quote = self.bytes[self.offset];
        let delimiter = if self.bytes.get(self.offset..self.offset + 3) == Some(&[quote; 3]) {
            3
        } else {
            1
        };
        self.offset += delimiter;
        let content_start = self.offset;
        let mut escaped = false;
        let mut closed = false;
        while self.offset < self.bytes.len() {
            let is_delimiter = !escaped
                && self
                    .bytes
                    .get(self.offset..self.offset + delimiter)
                    .is_some_and(|candidate| candidate.iter().all(|byte| *byte == quote));
            if is_delimiter {
                closed = true;
                break;
            }
            let byte = self.bytes[self.offset];
            self.line += usize::from(byte == b'\n');
            escaped = if byte == b'\\' { !escaped } else { false };
            self.offset += 1;
        }
        let content_end = self.offset;
        if closed {
            self.offset += delimiter;
        }
        let raw = self
            .source
            .get(content_start..content_end)
            .unwrap_or_default();
        let value = (closed && !raw.contains('\\') && !raw.contains('$')).then(|| raw.to_owned());
        Token {
            kind: TokenKind::String(value),
            start,
            end: self.offset,
            line,
        }
    }

    fn scan_identifier(&mut self) -> Token {
        let start = self.offset;
        let line = self.line;
        self.offset += 1;
        while self
            .bytes
            .get(self.offset)
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$'))
        {
            self.offset += 1;
        }
        Token {
            kind: TokenKind::Identifier(self.source[start..self.offset].to_owned()),
            start,
            end: self.offset,
            line,
        }
    }
}

impl Iterator for Scanner<'_> {
    type Item = Token;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            self.skip_trivia();
            let byte = *self.bytes.get(self.offset)?;
            if matches!(byte, b'\'' | b'"') {
                return Some(self.scan_string());
            }
            if byte.is_ascii_alphabetic() || byte == b'_' {
                return Some(self.scan_identifier());
            }
            let start = self.offset;
            let line = self.line;
            self.offset += 1;
            if byte.is_ascii_punctuation() {
                return Some(Token {
                    kind: TokenKind::Symbol(byte as char),
                    start,
                    end: self.offset,
                    line,
                });
            }
        }
    }
}
