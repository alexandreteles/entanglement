// File-level regression fixture: comments do not add NLOC.

fn analyze(value: Option<i32>, first: bool, second: bool) -> Option<i32> {
    let Some(value) = value else {
        return None;
    };

    if first && second {
        match value {
            0 => Some(1),
            1 | 2 => Some(2),
            _ if first || second => Some(3),
            _ => Some(4),
        }
    } else {
        Some(value)?
    }
}

fn embedded() {
    let _markup = v!(<section><p>hello</p></section>);
}
