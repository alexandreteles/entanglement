"""Exercise the Entanglement snapshot comparator through its command-line interface."""

import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


SKILL_ROOT = Path(__file__).resolve().parents[1]
COMPARATOR = SKILL_ROOT / 'scripts' / 'compare.py'


def metric_file(
    path: Path,
    source_hash: str,
    score: int | float = 50.0,
    nloc: int = 10,
    cc: int = 1,
    cogc: int = 0,
    function_score: int | float = 50.0,
) -> dict[str, object]:
    """Build one report file with validated file and diagnostic function metrics."""
    rating = 'red_low' if score < 10 else 'yellow_moderate' if score < 20 else 'green_good'
    function_rating = (
        'red_low'
        if function_score < 10
        else 'yellow_moderate'
        if function_score < 20
        else 'green_good'
    )
    return {
        'path': str(path),
        'hash': source_hash,
        'nloc': nloc,
        'maintainability_index': {'score': score, 'rating': rating},
        'functions': [
            {
                'name': 'sample',
                'cyclomatic_complexity': cc,
                'cognitive_complexity': cogc,
                'maintainability_index': {
                    'score': function_score,
                    'rating': function_rating,
                },
            }
        ],
    }


class CompareCliTests(unittest.TestCase):
    """Validate comparator decisions and complete summaries as a caller sees them."""

    def setUp(self) -> None:
        """Create an isolated report workspace for each CLI test."""
        self._temporary_directory = tempfile.TemporaryDirectory()
        self.addCleanup(self._temporary_directory.cleanup)
        self.workspace = Path(self._temporary_directory.name)

    def write_report(self, name: str, files: list[dict[str, object]]) -> Path:
        """Write one Entanglement-shaped JSON report into the temporary workspace."""
        report = self.workspace / f'{name}.json'
        report.write_text(json.dumps({'files': files}, allow_nan=True), encoding='utf-8')
        return report

    def compare_reports(
        self,
        before: Path,
        after: Path,
        before_root: Path,
        after_root: Path,
    ) -> subprocess.CompletedProcess[str]:
        """Invoke the installed comparator with explicit roots and capture its result."""
        return subprocess.run(
            [
                sys.executable,
                str(COMPARATOR),
                str(before),
                str(after),
                '--before-root',
                str(before_root),
                '--after-root',
                str(after_root),
            ],
            capture_output=True,
            check=False,
            text=True,
        )

    def assert_summary(
        self,
        result: subprocess.CompletedProcess[str],
        expected_exit: int,
    ) -> dict[str, object]:
        """Assert the CLI exit code and decode its full JSON summary."""
        self.assertEqual(result.returncode, expected_exit, result.stderr)
        return json.loads(result.stdout)

    def test_unrounded_file_mi_boundary_is_a_hard_gate(self) -> None:
        """Reject 9.999 exactly and pass at the inclusive score of 10.0."""
        before_root = self.workspace / 'before'
        after_root = self.workspace / 'after'
        source_before = before_root / 'src' / 'sample.py'
        source_after = after_root / 'src' / 'sample.py'
        before = self.write_report('boundary-before', [metric_file(source_before, 'before', 12.0)])
        below = self.write_report(
            'boundary-below',
            [metric_file(source_after, 'below', 9.999, function_score=80.0)],
        )
        at_boundary = self.write_report(
            'boundary-at',
            [metric_file(source_after, 'at', 10.0, function_score=80.0)],
        )

        rejected = self.assert_summary(self.compare_reports(before, below, before_root, after_root), 1)
        accepted = self.assert_summary(
            self.compare_reports(before, at_boundary, before_root, after_root),
            0,
        )

        self.assertEqual(rejected['red_edited'], ['src/sample.py'])
        self.assertFalse(rejected['gate_passed'])
        self.assertEqual(accepted['red_edited'], [])
        self.assertTrue(accepted['gate_passed'])
        self.assertEqual(accepted['changes'][0]['after']['mi'], 10.0)

    def test_untouched_red_debt_passes_but_edited_red_file_fails(self) -> None:
        """Gate the file aggregate while reporting unrelated red debt separately."""
        before_root = self.workspace / 'baseline-root'
        after_root = self.workspace / 'candidate-root'
        before = self.write_report(
            'debt-before',
            [
                metric_file(before_root / 'legacy.py', 'legacy', 8.0),
                metric_file(before_root / 'task.py', 'task-before', 35.0),
            ],
        )
        safe_after = self.write_report(
            'debt-safe-after',
            [
                metric_file(after_root / 'legacy.py', 'legacy', 8.0),
                metric_file(after_root / 'task.py', 'task-after', 34.0),
            ],
        )
        red_after = self.write_report(
            'debt-red-after',
            [
                metric_file(after_root / 'legacy.py', 'legacy', 8.0),
                metric_file(
                    after_root / 'task.py',
                    'task-red',
                    9.5,
                    function_score=85.0,
                ),
            ],
        )

        safe = self.assert_summary(self.compare_reports(before, safe_after, before_root, after_root), 0)
        red = self.assert_summary(self.compare_reports(before, red_after, before_root, after_root), 1)

        self.assertEqual(safe['untouched_red'], ['legacy.py'])
        self.assertEqual(safe['edited'], ['task.py'])
        self.assertTrue(safe['gate_passed'])
        self.assertEqual(red['red_edited'], ['task.py'])
        self.assertEqual(red['changes'][0]['after']['mi'], 9.5)
        self.assertEqual(red['changes'][0]['after']['cc'], 1)
        self.assertFalse(red['gate_passed'])

    def test_added_deleted_and_renamed_paths_compare_across_roots(self) -> None:
        """Match unchanged paths across copies and express rename as add plus removal."""
        before_root = self.workspace / 'before-copy'
        after_root = self.workspace / 'after-copy'
        before = self.write_report(
            'paths-before',
            [
                metric_file(before_root / 'src' / 'stable.py', 'stable', 30.0),
                metric_file(before_root / 'src' / 'old_name.py', 'renamed-bytes', 35.0),
                metric_file(before_root / 'src' / 'removed.py', 'removed', 40.0),
            ],
        )
        after = self.write_report(
            'paths-after',
            [
                metric_file(after_root / 'src' / 'stable.py', 'stable', 30.0),
                metric_file(after_root / 'src' / 'new_name.py', 'renamed-bytes', 35.0),
                metric_file(after_root / 'src' / 'added.py', 'added', 45.0),
            ],
        )

        result = self.assert_summary(self.compare_reports(before, after, before_root, after_root), 0)

        self.assertEqual(result['added'], ['src/added.py', 'src/new_name.py'])
        self.assertEqual(result['removed'], ['src/old_name.py', 'src/removed.py'])
        self.assertEqual(result['edited'], ['src/added.py', 'src/new_name.py'])
        self.assertEqual(result['files_before'], 3)
        self.assertEqual(result['files_after'], 3)
        self.assertEqual(result['totals_before'], {'nloc': 30, 'cc': 3, 'cogc': 0})
        self.assertEqual(result['totals_after'], {'nloc': 30, 'cc': 3, 'cogc': 0})
        changes = {item['path']: item for item in result['changes']}
        self.assertIsNone(changes['src/old_name.py']['after'])
        self.assertIsNone(changes['src/removed.py']['after'])
        self.assertIsNone(changes['src/new_name.py']['before'])

    def test_unchanged_hash_recursion_cognitive_delta_is_reported(self) -> None:
        """Surface a caller CogC increase from recursion without a source hash change."""
        before_root = self.workspace / 'snapshot-a'
        after_root = self.workspace / 'snapshot-b'
        before = self.write_report(
            'context-before',
            [metric_file(before_root / 'pkg' / 'caller.go', 'same-bytes', 42.0, cogc=2)],
        )
        after = self.write_report(
            'context-after',
            [metric_file(after_root / 'pkg' / 'caller.go', 'same-bytes', 42.0, cogc=3)],
        )

        result = self.assert_summary(self.compare_reports(before, after, before_root, after_root), 0)

        self.assertEqual(result['edited'], [])
        self.assertEqual(result['contextual_changes'], ['pkg/caller.go'])
        self.assertEqual(result['delta'], {'nloc': 0, 'cc': 0, 'cogc': 1})
        self.assertEqual(result['changes'][0]['before']['hash'], 'same-bytes')
        self.assertEqual(result['changes'][0]['after']['cogc'], 3)

    def test_missing_metrics_and_nonfinite_mi_return_incomplete_exit(self) -> None:
        """Reject absent aggregate metrics, absent function fields, and nonfinite MI."""
        before_root = self.workspace / 'old'
        after_root = self.workspace / 'new'
        baseline = self.write_report(
            'valid-before',
            [metric_file(before_root / 'file.py', 'old', 40.0)],
        )
        missing_mi_file = metric_file(after_root / 'file.py', 'new', 40.0)
        del missing_mi_file['maintainability_index']
        missing_mi = self.write_report('missing-mi', [missing_mi_file])
        missing_cogc_file = metric_file(after_root / 'file.py', 'new', 40.0)
        del missing_cogc_file['functions'][0]['cognitive_complexity']
        missing_cogc = self.write_report('missing-cogc', [missing_cogc_file])
        nonfinite = self.write_report(
            'nonfinite-mi',
            [metric_file(after_root / 'file.py', 'new', float('inf'))],
        )

        for report in (missing_mi, missing_cogc, nonfinite):
            with self.subTest(report=report.name):
                result = self.compare_reports(baseline, report, before_root, after_root)
                self.assertEqual(result.returncode, 2)
                self.assertIn('Incomplete metric comparison:', result.stderr)
                self.assertEqual(result.stdout, '')

    def test_two_empty_reports_are_incomplete(self) -> None:
        """Reject an empty inventory on both sides instead of passing vacuously."""
        before_root = self.workspace / 'empty-before'
        after_root = self.workspace / 'empty-after'
        before = self.write_report('empty-before-report', [])
        after = self.write_report('empty-after-report', [])

        result = self.compare_reports(before, after, before_root, after_root)

        self.assertEqual(result.returncode, 2)
        self.assertIn('no supported files measured in either snapshot', result.stderr)
        self.assertEqual(result.stdout, '')

    def test_deleting_every_file_passes_and_reports_removed_totals(self) -> None:
        """Allow an empty after inventory when it represents a measured deletion."""
        before_root = self.workspace / 'repo-before'
        after_root = self.workspace / 'repo-after'
        before = self.write_report(
            'delete-before',
            [metric_file(before_root / 'only.go', 'old', 15.0, nloc=7, cc=2, cogc=3)],
        )
        after = self.write_report('delete-after', [])

        result = self.assert_summary(self.compare_reports(before, after, before_root, after_root), 0)

        self.assertEqual(result['files_before'], 1)
        self.assertEqual(result['files_after'], 0)
        self.assertEqual(result['removed'], ['only.go'])
        self.assertEqual(result['delta'], {'nloc': -7, 'cc': -2, 'cogc': -3})
        self.assertIsNone(result['changes'][0]['after'])


if __name__ == '__main__':
    unittest.main()
