use super::Replacement;
use crate::model::SemanticStore;
use std::ops::Range;

#[derive(Default)]
pub(crate) struct Projection {
    pub bytes: Vec<u8>,
    spans: Vec<(Range<usize>, Range<usize>)>,
}

impl Projection {
    /// Replace known declaration macros. Preserve an original range for each
    /// generated spelling, including empty and multi-token replacements.
    pub fn new<'a>(source: &[u8], replacements: impl Iterator<Item = &'a Replacement>) -> Self {
        let mut projection = Self::default();
        let mut previous = 0;
        for item in replacements {
            let Some(text) = &item.text else { continue };
            projection
                .bytes
                .extend_from_slice(&source[previous..item.range.start]);
            let start = projection.bytes.len();
            if text.is_empty() {
                projection.bytes.push(b' ')
            } else {
                projection.bytes.extend_from_slice(text.as_bytes())
            }
            projection
                .spans
                .push((start..projection.bytes.len(), item.range.clone()));
            previous = item.range.end;
        }
        projection.bytes.extend_from_slice(&source[previous..]);
        projection
    }

    pub fn changed(&self) -> bool {
        !self.spans.is_empty()
    }

    pub fn mapping_key(&self) -> Vec<(usize, usize, usize, usize)> {
        self.spans
            .iter()
            .map(|(generated, written)| {
                (generated.start, generated.end, written.start, written.end)
            })
            .collect()
    }

    pub fn offset(&self, offset: usize, end: bool) -> usize {
        let passed = self
            .spans
            .partition_point(|(written, _)| written.end <= offset);
        if let Some((written, original)) = self.spans.get(passed)
            && written.contains(&offset)
        {
            return if end && offset > written.start {
                original.end
            } else {
                original.start
            };
        }
        self.spans
            .get(passed.wrapping_sub(1))
            .map_or(offset, |(written, original)| {
                original.end + (offset - written.end)
            })
    }

    pub fn range(&self, start: &mut usize, end: &mut usize) {
        *start = self.offset(*start, false);
        *end = self.offset(*end, true);
    }

    pub fn semantic(&self, store: &mut SemanticStore) {
        if !self.changed() {
            return;
        }
        for item in &mut store.definitions {
            self.range(&mut item.start_byte, &mut item.end_byte);
            self.range(&mut item.scope_start, &mut item.scope_end);
        }
        for item in &mut store.references {
            self.range(&mut item.start_byte, &mut item.end_byte);
            self.range(&mut item.scope_start, &mut item.scope_end);
            item.call_owner = item.call_owner.map(|at| self.offset(at, false));
            item.execution_owner = item.execution_owner.map(|at| self.offset(at, false));
        }
        for item in &mut store.locals {
            self.range(&mut item.start_byte, &mut item.end_byte);
            self.range(&mut item.scope_start, &mut item.scope_end);
        }
        if let Some(data) = store.semantic.c_family.as_deref_mut() {
            data.symbols = std::mem::take(&mut data.symbols)
                .into_iter()
                .map(|((at, name), symbol)| ((self.offset(at, false), name), symbol))
                .collect();
            data.calls = std::mem::take(&mut data.calls)
                .into_iter()
                .map(|(at, call)| (self.offset(at, false), call))
                .collect();
            for (start, end, _, _) in data.imports.iter_mut() {
                self.range(start, end);
            }
            for (start, end, _, _) in data.using.iter_mut() {
                self.range(start, end);
            }
            for (start, end, _) in data.aliases.values_mut() {
                self.range(start, end);
            }
            for (start, end) in &mut data.errors {
                self.range(start, end);
            }
        }
        for record in &mut store.semantic.structure {
            self.range(&mut record.start, &mut record.end);
            record.body_start = record.body_start.map(|at| self.offset(at, false));
            for (start, end) in &mut record.type_ranges {
                self.range(start, end);
            }
            for relation in &mut record.relations {
                self.range(&mut relation.start, &mut relation.end);
            }
        }
    }
}
