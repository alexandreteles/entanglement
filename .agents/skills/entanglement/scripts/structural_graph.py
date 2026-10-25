"""Validate exact indexed structural graph results."""

import math

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

