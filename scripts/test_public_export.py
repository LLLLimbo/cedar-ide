#!/usr/bin/env python3
"""Regression checks for clean export and recovery from an existing public tree."""
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

EXPORTER = Path(__file__).with_name('export_public_checkpoints.py')
FOOTER = 'Public source history and omitted machine-specific evidence are described in [PUBLICATION.md](PUBLICATION.md).'


def git(root, *arguments):
    return subprocess.check_output(['git', '-C', str(root), *arguments], stderr=subprocess.DEVNULL)


class PublicExportTests(unittest.TestCase):
    def fixture(self, root, published):
        (root / 'scripts').mkdir(parents=True)
        shutil.copyfile(EXPORTER, root / 'scripts/export_public_checkpoints.py')
        (root / 'docs').mkdir()
        (root / 'docs/run.txt').write_text('synthetic omitted evidence\n')
        note = ('\n> Public-source note: existing note.\n' if published else '')
        (root / 'docs/REPORT.md').write_text('# Report\n' + note + '\n[log](run.txt)\n')
        (root / 'README.md').write_text('# Example\n' + ('\n' + FOOTER + '\n' if published else ''))
        if published:
            (root / 'PUBLICATION.md').write_text('Obsolete generated publication\n')
        git(root, 'init', '-q')
        git(root, 'add', '.')
        git(root, '-c', 'user.name=Export test', '-c', 'user.email=test@localhost',
            'commit', '-qm', 'Synthetic export fixture')
        git(root, 'tag', 'test-stage')

    def check_export(self, published):
        with tempfile.TemporaryDirectory(prefix='cedar-export-test-') as directory:
            base = Path(directory)
            source = base / 'source'
            self.fixture(source, published)
            destination = base / 'export'
            subprocess.run([sys.executable, str(source / 'scripts/export_public_checkpoints.py'),
                            '--destination', str(destination), '--tags', 'test-stage'],
                           check=True, stdout=subprocess.DEVNULL, timeout=30)
            stage = json.loads((destination / 'publish-manifest.json').read_text())['stages'][0]
            paths = [item['path'] for item in stage['files']]
            self.assertEqual(len(paths), len(set(paths)))
            self.assertEqual(paths.count('PUBLICATION.md'), 1)
            self.assertNotIn('docs/run.txt', paths)
            exported = Path(stage['directory'])
            self.assertNotIn('Obsolete generated', (exported / 'PUBLICATION.md').read_text())
            self.assertEqual((exported / 'README.md').read_text().count(FOOTER), 1)
            report = (exported / 'docs/REPORT.md').read_text()
            self.assertEqual(report.count('> Public-source note:'), 1)
            self.assertIn('(../PUBLICATION.md#verification-evidence)', report)
            git(exported, 'init', '-q')
            git(exported, 'add', '.')
            self.assertEqual(git(exported, 'write-tree').decode().strip(), stage['expected_tree_sha'])

    def test_initial_source_exports_one_publication_and_evidence_note(self):
        self.check_export(False)

    def test_recovered_public_source_does_not_duplicate_paths_or_notes(self):
        self.check_export(True)


if __name__ == '__main__':
    unittest.main()
