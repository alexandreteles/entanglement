//! Select C# preprocessor branches while retaining physical offsets.
use crate::languages::syntax::named_children;
use std::collections::BTreeSet;
use std::ops::Range;
use tree_sitter::Node;

#[derive(Clone, Debug)]
pub(crate) struct Line {
    pub range: Range<usize>,
    pub row: usize,
    pub command: String,
    pub argument: String,
}

/// Find directives outside comments and literals, including recovered directive nodes.
pub(crate) fn lines(root: Node<'_>, source: &[u8]) -> Vec<Line> {
    if !source.contains(&b'#') {
        return Vec::new();
    }
    let mut protected = Vec::new();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if matches!(
            node.kind(),
            "comment"
                | "raw_string_literal"
                | "verbatim_string_literal"
                | "string_literal"
                | "interpolation_string_content"
                | "character_literal"
        ) {
            protected.push(node.byte_range());
        } else {
            stack.extend(named_children(node));
        }
    }
    protected.sort_unstable_by_key(|range| range.start);
    let mut start = 0;
    source
        .split_inclusive(|b| *b == b'\n')
        .enumerate()
        .filter_map(|(row, line)| {
            let offset = start;
            start += line.len();
            let trimmed = line.trim_ascii_start();
            let hash = offset + line.len() - trimmed.len();
            let tail = trimmed.strip_prefix(b"#")?.trim_ascii_start();
            let idx = protected.partition_point(|r| r.start <= hash);
            if idx > 0 && protected[idx - 1].contains(&hash) {
                return None;
            }
            let length = tail.iter().take_while(|b| b.is_ascii_alphabetic()).count();
            let command = std::str::from_utf8(&tail[..length]).ok()?;
            if !matches!(
                command,
                "define" | "undef" | "if" | "elif" | "else" | "endif" | "r" | "load"
            ) {
                return None;
            }
            let argument = String::from_utf8_lossy(&tail[length..]).trim().to_owned();
            Some(Line {
                range: hash..offset + line.trim_ascii_end().len(),
                row,
                command: command.into(),
                argument,
            })
        })
        .collect()
}

#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Mask {
    pub hidden: Vec<RangeKey>,
    pub uncertain: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct RangeKey(usize, usize);

impl Mask {
    pub fn active(&self, at: usize) -> bool {
        let i = self.hidden.partition_point(|r| r.1 <= at);
        !self.hidden.get(i).is_some_and(|r| r.0 <= at)
    }
    pub fn bytes(&self, source: &[u8]) -> Vec<u8> {
        let mut bytes = source.to_vec();
        for range in &self.hidden {
            blank(&mut bytes[range.0..range.1]);
        }
        bytes
    }
}

pub(crate) fn blank(bytes: &mut [u8]) {
    for byte in bytes {
        if !matches!(*byte, b'\r' | b'\n') {
            *byte = b' ';
        }
    }
}

pub(crate) fn mask(lines: &[Line], size: usize, symbols: &BTreeSet<String>) -> Mask {
    let mut symbols = symbols.clone();
    let mut result = Mask::default();
    let mut active = true;
    let mut stack = Vec::new();
    let mut last = 0;
    for line in lines {
        if !active {
            result.hidden.push(RangeKey(last, line.range.start));
        }
        let input = line.argument.split("//").next().unwrap_or("").trim();
        let mut value = || {
            boolean(input, &symbols).unwrap_or_else(|| {
                result.uncertain = true;
                false
            })
        };
        match line.command.as_str() {
            "define" if active => {
                symbols.insert(input.into());
            }
            "undef" if active => {
                symbols.remove(input);
            }
            "if" => {
                let test = value();
                stack.push((active, test));
                active &= test;
            }
            "elif" => {
                if let Some((parent, taken)) = stack.last_mut() {
                    let test = value();
                    active = *parent && !*taken && test;
                    *taken |= test;
                } else {
                    result.uncertain = true;
                }
            }
            "else" => {
                if let Some((parent, taken)) = stack.last_mut() {
                    active = *parent && !*taken;
                    *taken = true;
                } else {
                    result.uncertain = true;
                }
            }
            "endif" => {
                if let Some((parent, _)) = stack.pop() {
                    active = parent;
                } else {
                    result.uncertain = true;
                }
            }
            _ => {}
        }
        result
            .hidden
            .push(RangeKey(line.range.start, line.range.end));
        last = line.range.end;
    }
    if !active {
        result.hidden.push(RangeKey(last, size));
    }
    result.uncertain |= !stack.is_empty();
    result
}

/// Cover written branches for physical metrics, including branches absent from every build.
pub(crate) fn coverage(lines: &[Line], size: usize, configured: &BTreeSet<Mask>) -> BTreeSet<Mask> {
    let mut masks = configured.clone();
    let mut branches = Vec::<(usize, usize)>::new();
    for (index, line) in lines.iter().enumerate() {
        match line.command.as_str() {
            "if" => branches.push((index, index)),
            "else" | "elif" => {
                if let Some(branch) = branches.last_mut() {
                    branch.1 = index;
                }
            }
            "endif" => {
                branches.pop();
            }
            _ => continue,
        }
        let at = line.range.end;
        if line.command == "endif" || masks.iter().any(|m| m.active(at)) {
            continue;
        }
        let mut forced = lines.to_vec();
        for (first, selected) in &branches {
            for (i, item) in forced
                .iter_mut()
                .enumerate()
                .take(selected + 1)
                .skip(*first)
            {
                if matches!(item.command.as_str(), "if" | "elif") {
                    item.argument = (i == *selected).to_string();
                }
            }
        }
        masks.insert(mask(&forced, size, &BTreeSet::new()));
    }
    masks
}

fn boolean(input: &str, symbols: &BTreeSet<String>) -> Option<bool> {
    let input = input.trim();
    for operation in ["||", "&&", "!=", "=="] {
        let mut depth = 0i32;
        for (index, ch) in input.char_indices().rev() {
            match ch {
                ')' => depth += 1,
                '(' => depth -= 1,
                _ if depth == 0 && input[index..].starts_with(operation) => {
                    let left = boolean(&input[..index], symbols)?;
                    let right = boolean(&input[index + 2..], symbols)?;
                    return Some(match operation {
                        "||" => left || right,
                        "&&" => left && right,
                        "!=" => left != right,
                        _ => left == right,
                    });
                }
                _ => {}
            }
        }
    }
    if let Some(inner) = input.strip_prefix('!') {
        return boolean(inner, symbols).map(|v| !v);
    }
    if let Some(inner) = input.strip_prefix('(').and_then(|s| s.strip_suffix(')')) {
        return boolean(inner, symbols);
    }
    if input.is_empty() || !input.chars().all(|c| c == '_' || c.is_alphanumeric()) {
        return None;
    }
    Some(input == "true" || input != "false" && symbols.contains(input))
}
