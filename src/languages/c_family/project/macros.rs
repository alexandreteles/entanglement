use super::*;
use crate::languages::c_family::lex::{self, Kind};

pub(super) struct Word {
    range: Range<usize>,
    name: String,
    followed_by_paren: bool,
}

pub(super) fn words(source: &[u8]) -> Vec<Word> {
    let mut tokens = lex::tokens(source)
        .filter(|token| {
            let rest = &source[token.range.start..];
            token.kind != Kind::Comment
                && !rest.starts_with(b"\\\n")
                && !rest.starts_with(b"\\\r\n")
        })
        .peekable();
    let mut words = Vec::new();
    while let Some(token) = tokens.next() {
        if token.kind == Kind::Word {
            words.push(Word {
                name: String::from_utf8_lossy(&source[token.range.clone()]).into_owned(),
                range: token.range,
                followed_by_paren: tokens
                    .peek()
                    .is_some_and(|next| &source[next.range.clone()] == b"("),
            });
        }
    }
    words
}

impl FileView {
    pub(super) fn record_uses(
        &mut self,
        words: &[Word],
        range: &Range<usize>,
        env: &Environment,
        origins: &BTreeMap<String, Arc<MacroSite>>,
        repeated: bool,
    ) {
        let first = words.partition_point(|word| word.range.start < range.start);
        for word in words[first..]
            .iter()
            .take_while(|word| word.range.start < range.end)
        {
            let expanded = match env.macros.get(&word.name) {
                Some(Macro::Function) => word.followed_by_paren,
                Some(Macro::Undefined) | None => false,
                _ => true,
            };
            if !expanded && !repeated {
                continue;
            }
            let origin = origins.get(&word.name).filter(|_| expanded).cloned();
            if let Some(previous) = self.uses.get_mut(&word.range.start) {
                if !expanded || previous.text != view::replacement(&word.name, env) {
                    previous.text = None;
                }
                if !previous.origins.contains(&origin) {
                    previous.origins.push(origin);
                }
            } else if expanded {
                let mut origins = vec![origin];
                if repeated && origins[0].is_some() {
                    origins.push(None);
                }
                self.uses.insert(
                    word.range.start,
                    Replacement {
                        range: word.range.clone(),
                        text: if repeated {
                            None
                        } else {
                            view::replacement(&word.name, env)
                        },
                        origins,
                    },
                );
            }
        }
    }
}
