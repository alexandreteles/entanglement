"""Compare dependencies, matched reachability, and uncertain identities."""

def reached(graph: dict[str, object], matched: set[str]) -> set[tuple[str, str]]:
    """Read matched endpoint pairs from exact full-graph reachability results."""
    units = graph['units']
    return {(units[impact['unit']], units[target])
            for impact in graph['impacts'] if units[impact['unit']] in matched
            for target in impact['dependencies'] if units[target] in matched}

def semantic(value: object) -> object:
    """Remove snapshot-local source positions before comparing declaration facts."""
    match value:
        case dict():
            return {key: semantic(item) for key, item in value.items()
                    if key not in {'start', 'end', 'start_byte', 'end_byte', 'range', 'evidence'}
                    and not (key == 'text' and value.get('children'))}
        case list():
            return [semantic(item) for item in value]
        case _:
            return value

def edge_keys(graph: dict[str, object]) -> set[tuple[str, str, str]]:
    """Return dependency identities without snapshot-local evidence offsets."""
    return {(edge['source'], edge['target'], edge['kind']) for edge in graph.get('edges', [])}

def ordered_targets(graph: dict[str, object]) -> dict[str, list[str]]:
    """Read adapter-marked dependency order, without comparing source positions."""
    sources = {}
    for edge in graph.get('edges', []):
        if not edge.get('ordered', False):
            continue
        starts = [item['start'] for item in edge['evidence'] if item['detail'] != 'relation']
        sources.setdefault(edge['source'], []).append((min(starts, default=0), edge['target']))
    return {source: list(dict.fromkeys(target for _, target in sorted(targets)))
            for source, targets in sources.items()}

def graph_delta(old: dict[str, object], new: dict[str, object], complete: bool) -> dict[str, object]:
    """Compare one projection using matched endpoints and full-graph reachability."""
    matched = set(old['units']) & set(new['units'])
    old_reach, new_reach = reached(old, matched), reached(new, matched)
    old_cycles = {tuple(cycle['members']) for cycle in old['cycles']}
    new_cycles = {tuple(cycle['members']) for cycle in new['cycles']}
    old_edges, new_edges = edge_keys(old), edge_keys(new)
    old_order, new_order = ordered_targets(old), ordered_targets(new)
    return {
        'granularity': new['granularity'], 'projection': new['projection'],
        'pc_before': old['pc'], 'pc_after': new['pc'],
        'cycles_added': sorted(new_cycles - old_cycles),
        'cycles_removed': sorted(old_cycles - new_cycles) if complete else None,
        'reach_added': sorted(new_reach - old_reach),
        'reach_removed': sorted(old_reach - new_reach) if complete else None,
        'edges_added': sorted(new_edges - old_edges),
        'edges_removed': sorted(old_edges - new_edges) if complete else None,
        'order_changed': [{'unit': source, 'before': old_order[source], 'after': new_order[source]}
                          for source in sorted(old_order.keys() & new_order.keys() & matched)
                          if old_order[source] != new_order[source]
                          and set(old_order[source]) == set(new_order[source])],
    }

def uncertain_matches(before: dict[str, object], after: dict[str, object]) -> list[list[str]]:
    """Report possible renames without treating file hashes as declaration proof."""
    old = {unit['id']: unit for unit in before.get('units', [])}
    new = {unit['id']: unit for unit in after.get('units', [])}
    return sorted([old_id, new_id]
                  for old_id in old.keys() - new.keys() for new_id in new.keys() - old.keys()
                  if rename_evidence(old[old_id], new[new_id]))

def rename_evidence(old: dict[str, object], new: dict[str, object]) -> bool:
    """Check source or complete syntax evidence while preserving uncertain identity."""
    if old['kind'] != new['kind']:
        return False
    shape = old.get('structural_fingerprint')
    same_shape = shape is not None and shape == new.get('structural_fingerprint')
    source = old.get('source_hash')
    same_source = (source is not None and source == new.get('source_hash')
                   and (old['kind'] == 'module' or old['name'] == new['name']))
    return same_shape or same_source

