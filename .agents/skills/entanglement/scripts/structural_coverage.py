"""Compare source coverage without claiming improvement from lost evidence."""

from collections import Counter

def coverage_loss(before: dict[str, object], after: dict[str, object]) -> list[str]:
    """Report lost context evidence. Explicit candidate deletions are permitted."""
    previous = {(item['path'], item['language'], item.get('context')): item for item in before['coverage']}
    current = {(item['path'], item['language'], item.get('context')): item for item in after['coverage']}
    removed = set(after.get('removed_paths', []))
    lost = []
    for key, old in previous.items():
        if key[0] in removed:
            continue
        new = current.get(key)
        if reduced(old, new):
            lost.append(key[0])
    previous_uncertain, current_uncertain = uncertain(before), uncertain(after)
    lost.extend(path for path, count in current_uncertain.items() if count > previous_uncertain[path])
    old_resolved = support(before, 'resolved')
    lost.extend(key[0] for key in (old_resolved & support(after, 'external')) - support(after, 'resolved'))
    return sorted(set(lost))

def reduced(old: dict[str, object], new: dict[str, object] | None) -> bool:
    """Detect missing contexts, lower coverage states, and new limits."""
    if new is None:
        return True
    rank = {'complete': 2, 'partial': 1, 'unavailable': 0, 'inapplicable': 0}
    return (rank[new['status']] < rank[old['status']]
            or rank[new.get('effective', new['status'])] < rank[old.get('effective', old['status'])]
            or bool(set(new['limitations']) - set(old['limitations'])))

def uncertain(state: dict[str, object]) -> Counter[str]:
    """Count unresolved source evidence so lost resolution cannot show improvement."""
    result = Counter()
    for edge in state.get('relationships', []):
        if edge['resolution'] in {'unresolved', 'ambiguous'}:
            evidence = edge.get('evidence', [])
            result[edge['source']] += max(len(evidence), 1)
    return result

def support(state: dict[str, object], status: str) -> set[tuple[str, str, str, str]]:
    """Identify unchanged written references without comparing byte offsets.

    A known internal reference that becomes external loses project evidence.
    Generic relation markers and resource URL markers cannot identify a
    written reference. Other support keys do not use byte offsets.
    """
    return {(edge['source'], edge['kind'], evidence['owner'], evidence['detail'])
            for edge in state.get('relationships', [])
            if edge['resolution'] == status
            for evidence in edge.get('evidence', [])
            if evidence['detail'] not in {'relation', 'embed_ptr'}
            and not (edge.get('kind') == 'resource' and evidence['detail'] == 'url')}

