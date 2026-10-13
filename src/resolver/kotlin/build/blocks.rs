use super::lexer::{Token, ident_at, matching_open, symbol_at, token_identifier};
use super::model::GradleBlock;
use super::syntax::KotlinSyntax;

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
    let name = literal_string(tokens, paren + 1, close, syntax)?;
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

fn literal_string(
    tokens: &[Token],
    start: usize,
    end: usize,
    syntax: &KotlinSyntax,
) -> Option<String> {
    super::lexer::literal_string(tokens, start, end, |token| syntax.permits_string(token))
}

pub(super) fn valid_source_set_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
}
