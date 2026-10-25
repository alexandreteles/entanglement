use tree_sitter::Node;

/// Hash the complete syntax shape and qualified owner for uncertain matching.
///
/// Comments, source positions, and formatting are absent. Only the declared
/// name in the shared matcher's header range is masked. Body names and literal
/// text remain evidence; one moved member cannot identify its container.
pub(super) fn syntax(
    root: Node<'_>,
    name: Option<Node<'_>>,
    source: &[u8],
    owners: &[String],
) -> String {
    let mut fingerprint = blake3::Hasher::new();
    for owner in owners {
        append(&mut fingerprint, owner.as_bytes());
    }
    let header_end = root.start_byte()
        + crate::report_delta::header_end(&source[root.start_byte()..root.end_byte()]);
    let name = name
        .filter(|name| name.start_byte() < header_end)
        .map(|name| name.id());
    let mut pending = vec![(root, false)];
    while let Some((node, expanded)) = pending.pop() {
        if node.kind().contains("comment") {
            continue;
        }
        if expanded {
            fingerprint.update(&[255]);
            continue;
        }
        append(&mut fingerprint, node.kind().as_bytes());
        if node.child_count() == 0 {
            append(
                &mut fingerprint,
                if Some(node.id()) == name {
                    b"@declaration"
                } else {
                    &source[node.start_byte()..node.end_byte()]
                },
            );
        }
        pending.push((node, true));
        let mut cursor = node.walk();
        pending.extend(
            node.children(&mut cursor)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .map(|child| (child, false)),
        );
    }
    fingerprint.finalize().to_hex().to_string()
}

fn append(fingerprint: &mut blake3::Hasher, bytes: &[u8]) {
    fingerprint.update(&(bytes.len() as u64).to_le_bytes());
    fingerprint.update(bytes);
}
