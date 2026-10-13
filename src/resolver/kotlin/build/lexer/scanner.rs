use super::{Token, TokenKind};

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
