"""Report advisory structural changes while preserving the ordinary MI exit."""

import json
from pathlib import Path

from structural_coverage import coverage_loss
from structural_delta import graph_delta, semantic, uncertain_matches
from structural_graph import graphs


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

def unavailable(reason: str) -> dict[str, object]:
    """Report missing or invalid optional evidence without changing the MI gate."""
    return {'status': 'unavailable', 'reason': reason, 'review': True}


def compare(before_path: Path, after_path: Path, before_root: Path, after_root: Path,
            requested: bool = False) -> dict[str, object] | None:
    """Read optional structure and return a review result, never a gate decision.

    Legacy reports can omit both structural payloads unless review is requested.
    Missing, invalid, local, or unreadable structural data is advisory. Required
    ordinary metrics must be validated separately before this function is called.
    """
    try:
        before = json.loads(before_path.read_text()).get('structure')
        after = json.loads(after_path.read_text()).get('structure')
        if before is None and after is None and not requested:
            return None
        if before is None or after is None:
            return unavailable('missing')
        if not isinstance(before, dict) or not isinstance(after, dict):
            return unavailable('invalid')
        if before.get('scope') != 'repo' or after.get('scope') != 'repo':
            return unavailable('scope')
        return compare_states(normalize(before, before_root), normalize(after, after_root))
    except Exception:
        return unavailable('invalid')


def compare_states(before: dict[str, object], after: dict[str, object]) -> dict[str, object]:
    """Compare valid repository graphs and suppress removal claims when uncertain."""
    old_graphs, new_graphs = graphs(before), graphs(after)
    losses = coverage_loss(before, after)
    uncertain = uncertain_matches(before, after)
    summaries = [graph_delta(old, new_graphs[key], not losses and not uncertain)
                 for key, old in sorted(old_graphs.items())]
    effective_changed = semantic(before['effective']) != semantic(after['effective'])
    changes = ('cycles_added', 'cycles_removed', 'reach_added', 'reach_removed',
               'edges_added', 'edges_removed', 'order_changed')
    review = bool(losses or uncertain) or effective_changed or any(
        item[field] for item in summaries for field in changes)
    reason = 'coverage' if losses else 'identity' if uncertain else 'changed' if review else 'same'
    return {'status': 'partial' if losses or uncertain else 'ok', 'reason': reason,
            'review': review, 'coverage_lost': losses, 'match_uncertain': uncertain,
            'effective_changed': effective_changed, 'graphs': summaries}
