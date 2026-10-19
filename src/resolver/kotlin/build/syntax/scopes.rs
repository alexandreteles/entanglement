use super::*;

pub(crate) struct GradleScopes {
    ancestors: Vec<Vec<GradleBlock>>,
}

impl GradleScopes {
    pub(crate) fn scan(tokens: &[Token], syntax: &KotlinSyntax) -> Self {
        let mut stack = Vec::new();
        let mut ancestors = Vec::with_capacity(tokens.len());
        for (index, token) in tokens.iter().enumerate() {
            if matches!(token.kind, TokenKind::Symbol('}')) {
                stack.pop();
            }
            ancestors.push(stack.clone());
            if matches!(token.kind, TokenKind::Symbol('{')) {
                stack.push(classify_block(tokens, index, syntax, &stack));
            }
        }
        Self { ancestors }
    }

    pub(crate) fn at(&self, index: usize) -> &[GradleBlock] {
        self.ancestors.get(index).map_or(&[], Vec::as_slice)
    }

    pub(crate) fn brace_depth(&self, index: usize) -> Option<usize> {
        self.ancestors.get(index).map(Vec::len)
    }
}

pub(crate) fn dependency_scope(
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

pub(crate) fn classify_block(
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
    let name = literal_string(tokens, paren + 1, close, |token| {
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

pub(crate) fn valid_source_set_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
}
