"""Compare structural reports without changing the MI gate."""

import json
import math
from collections import Counter
from pathlib import Path


def normalize(value: object, root: Path) -> object:
    """Make source identities relative to a report root, including nested evidence."""
    match value:
        case str():
            prefix = str(root) + '/'
            return value.replace(prefix, '')
        case list():
            return [normalize(item, root) for item in value]
        case dict():
            return {normalize(key, root): normalize(item, root) for key, item in value.items()}
        case _:
            return value


def graphs(state: dict[str, object]) -> dict[tuple[str, str], dict[str, object]]:
    """Validate the four graph universes and exact PC fractions.

    Raise ValueError for missing projections or invalid fractions. Each graph
    must use its own unit count as the denominator base.
    """
    result = {}
    for graph in state['graphs']:
        key = graph['granularity'], graph['projection']
        if key in result:
            raise ValueError('duplicate structural projection')
        result[key] = graph
        validate_pc(graph)
        validate_impacts(graph)
    expected = {(kind, projection) for kind in ('module', 'type') for projection in ('contract', 'full')}
    if result.keys() != expected:
        raise ValueError('missing structural projection')
    return result


def validate_pc(graph: dict[str, object]) -> None:
    """Reject invalid PC values, including a denominator from another universe."""
    numerator, denominator = graph['pc']['numerator'], graph['pc']['denominator']
    if (type(numerator) is not int or type(denominator) is not int
            or denominator != len(graph['units']) ** 2
            or not len(graph['units']) <= numerator <= denominator):
        raise ValueError('invalid structural PC fraction')
    value = graph['pc']['value']
    if denominator == 0:
        if value is not None:
            raise ValueError('empty structural PC must be null')
    elif type(value) not in (int, float) or not math.isclose(value, numerator / denominator):
        raise ValueError('structural PC disagrees with its fraction')


def reached(graph: dict[str, object], matched: set[str]) -> set[tuple[str, str]]:
    """Read matched endpoint pairs from exact full-graph reachability results."""
    units = graph['units']
    return {(units[impact['unit']], units[target])
            for impact in graph['impacts'] if units[impact['unit']] in matched
            for target in impact['dependencies'] if units[target] in matched}


def unit_index(value: object, size: int) -> int:
    """Return a valid graph-local index; reject booleans and nonintegers."""
    if type(value) is not int or not 0 <= value < size:
        raise ValueError('invalid structural unit index')
    return value


def index_set(values: object, size: int) -> set[int]:
    """Validate one exact endpoint list, including duplicate detection."""
    if not isinstance(values, list):
        raise ValueError('structural endpoints must be an array')
    result = {unit_index(value, size) for value in values}
    if len(result) != len(values):
        raise ValueError('duplicate structural unit index')
    return result


def validate_impacts(graph: dict[str, object]) -> None:
    """Validate complete indexed reachability rows, reverse sets, and PC counts."""
    size = len(graph['units'])
    if not isinstance(graph['units'], list) or any(not isinstance(unit, str) or not unit for unit in graph['units']):
        raise ValueError('structural units must be nonempty string identities')
    if len(set(graph['units'])) != size:
        raise ValueError('duplicate structural unit identity')
    rows = {}
    for impact in graph['impacts']:
        source = unit_index(impact['unit'], size)
        dependencies = index_set(impact['dependencies'], size)
        dependents = index_set(impact['dependents'], size)
        if source in rows or source in dependencies or source in dependents:
            raise ValueError('duplicate structural reachability row or self endpoint')
        validate_predecessors(impact['predecessors'], size, source, dependencies)
        rows[source] = dependencies, dependents
    if set(rows) != set(range(size)):
        raise ValueError('missing structural reachability row')
    if size + sum(len(dependencies) for dependencies, _ in rows.values()) != graph['pc']['numerator']:
        raise ValueError('structural reachability disagrees with PC')
    validate_reverse(rows)


def validate_reverse(rows: dict[int, tuple[set[int], set[int]]]) -> None:
    """Require forward and reverse endpoint sets to describe the same reachability."""
    for source, (dependencies, dependents) in rows.items():
        if (any(source not in rows[target][1] for target in dependencies)
                or any(source not in rows[target][0] for target in dependents)):
            raise ValueError('structural forward and reverse reachability disagree')


def validate_predecessors(pairs: object, size: int, source: int, dependencies: set[int]) -> None:
    """Validate a complete, rooted predecessor tree encoded as integer pairs."""
    if not isinstance(pairs, list):
        raise ValueError('structural predecessors must be an array')
    parents = {}
    for pair in pairs:
        if not isinstance(pair, list) or len(pair) != 2:
            raise ValueError('invalid structural predecessor pair')
        target, parent = (unit_index(value, size) for value in pair)
        if target in parents:
            raise ValueError('duplicate structural predecessor')
        parents[target] = parent
    if set(parents) != dependencies | {source} or parents.get(source) != source:
        raise ValueError('incomplete structural predecessor tree')
    if not rooted(parents, source):
        raise ValueError('structural predecessor tree is disconnected')


def rooted(parents: dict[int, int], source: int) -> bool:
    """Return whether every predecessor belongs to the source's tree."""
    children = {}
    for target, parent in parents.items():
        if target != source:
            children.setdefault(parent, []).append(target)
    seen, pending = set(), [source]
    while pending:
        current = pending.pop()
        seen.add(current)
        pending.extend(children.get(current, []))
    return seen == set(parents)


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
    Resource URLs use their own boundary rules and are excluded here.
    """
    return {(edge['source'], edge['kind'], evidence['owner'], evidence['detail'])
            for edge in state.get('relationships', [])
            if edge['resolution'] == status and edge.get('kind') != 'resource'
            for evidence in edge.get('evidence', [])}


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


def compare(before_path: Path, after_path: Path, before_root: Path, after_root: Path,
            required: bool = False) -> dict[str, object] | None:
    """Compare graph membership, matched reachability, coverage, and declarations.

    Legacy metric reports can omit structure on both sides unless required.
    Missing structural evidence on one side raises ValueError. A lower PC
    value alone is not an improvement claim.
    """
    before = json.loads(before_path.read_text()).get('structure')
    after = json.loads(after_path.read_text()).get('structure')
    if before is None and after is None and not required:
        return None
    if before is None or after is None:
        raise ValueError('missing structural analysis')
    before, after = normalize(before, before_root), normalize(after, after_root)
    if before['scope'] != 'repo' or after['scope'] != 'repo':
        raise ValueError('structural comparison requires repository scope')
    old_graphs, new_graphs = graphs(before), graphs(after)
    losses = coverage_loss(before, after)
    uncertain = uncertain_matches(before, after)
    summaries = [graph_delta(old, new_graphs[key], not losses and not uncertain)
                 for key, old in sorted(old_graphs.items())]
    effective_changed = semantic(before['effective']) != semantic(after['effective'])
    changes = ('cycles_added', 'cycles_removed', 'reach_added', 'reach_removed', 'edges_added', 'edges_removed', 'order_changed')
    review = bool(losses or uncertain) or effective_changed or any(item[field] for item in summaries for field in changes)
    return {'review': review, 'coverage_lost': losses, 'match_uncertain': uncertain, 'effective_changed': effective_changed,
            'graphs': summaries}
