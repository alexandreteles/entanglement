use super::lex::{Kind, tokens};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Active {
    No,
    Maybe,
    Yes,
}

impl Active {
    pub fn and(self, other: Self) -> Self {
        match (self, other) {
            (Self::No, _) | (_, Self::No) => Self::No,
            (Self::Yes, Self::Yes) => Self::Yes,
            _ => Self::Maybe,
        }
    }
    pub fn or(self, other: Self) -> Self {
        match (self, other) {
            (Self::Yes, _) | (_, Self::Yes) => Self::Yes,
            (Self::No, Self::No) => Self::No,
            _ => Self::Maybe,
        }
    }
    pub fn inverse(self) -> Self {
        match self {
            Self::Yes => Self::No,
            Self::No => Self::Yes,
            Self::Maybe => Self::Maybe,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) enum Macro {
    Defined(String),
    Undefined,
    Unknown,
}

pub(super) struct Environment {
    pub macros: BTreeMap<String, Macro>,
    pub configured: bool,
}

impl Environment {
    pub fn defined(&self, name: &str) -> Active {
        match self.macros.get(name) {
            Some(Macro::Defined(_)) => Active::Yes,
            Some(Macro::Undefined) => Active::No,
            None if self.configured && !name.starts_with("__") => Active::No,
            _ => Active::Maybe,
        }
    }
    pub fn condition(&self, directive: &str, value: &str) -> Active {
        let value = value.as_bytes();
        let words: Vec<_> = tokens(value)
            .filter(|token| token.kind != Kind::Comment)
            .filter_map(|token| str::from_utf8(&value[token.range]).ok())
            .collect();
        match directive {
            "ifdef" | "elifdef" => self.defined(words.first().copied().unwrap_or("")),
            "ifndef" | "elifndef" => self.defined(words.first().copied().unwrap_or("")).inverse(),
            _ => self.evaluate(&words),
        }
    }

    fn evaluate(&self, words: &[&str]) -> Active {
        for op in ["||", "&&", "==", "!=", ">=", "<=", ">", "<"] {
            let mut depth = 0i32;
            if let Some(index) = words.iter().position(|&word| {
                depth += i32::from(word == "(") - i32::from(word == ")");
                depth == 0 && word == op
            }) {
                let (lhs, rest) = words.split_at(index);
                let rhs = &rest[1..];
                if op == "||" {
                    return self.evaluate(lhs).or(self.evaluate(rhs));
                }
                if op == "&&" {
                    return self.evaluate(lhs).and(self.evaluate(rhs));
                }
                let Some((a, b)) = self.number(lhs).zip(self.number(rhs)) else {
                    return Active::Maybe;
                };
                return active(match op {
                    "==" => a == b,
                    "!=" => a != b,
                    ">=" => a >= b,
                    "<=" => a <= b,
                    ">" => a > b,
                    _ => a < b,
                });
            }
        }
        match words {
            ["!", rest @ ..] => self.evaluate(rest).inverse(),
            ["(", rest @ .., ")"] => self.evaluate(rest),
            ["defined", name] | ["defined", "(", name, ")"] => self.defined(name),
            _ => self.number(words).map_or(Active::Maybe, |n| active(n != 0)),
        }
    }
    fn number(&self, words: &[&str]) -> Option<i64> {
        let [word] = words else { return None };
        let value = match self.macros.get(*word) {
            Some(Macro::Defined(value)) => value.as_str(),
            Some(Macro::Undefined) => return Some(0),
            Some(Macro::Unknown) => return None,
            None if self.defined(word) == Active::No
                && word.starts_with(|ch: char| ch.is_alphabetic() || ch == '_') =>
            {
                return Some(0);
            }
            None => word,
        };
        let digits = value.trim_end_matches(['u', 'U', 'l', 'L']);
        digits.strip_prefix("0x").map_or_else(
            || digits.parse().ok(),
            |hex| i64::from_str_radix(hex, 16).ok(),
        )
    }
}
fn active(value: bool) -> Active {
    if value { Active::Yes } else { Active::No }
}
