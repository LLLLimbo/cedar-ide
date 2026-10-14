#!/usr/bin/env python3
"""Regression checks for clean export and recovery from an existing public tree."""
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

EXPORTER = Path(__file__).with_name('export_public_checkpoints.py')
FONT_ROOT = Path('crates/app/assets/fonts')
FONT_SOURCE = EXPORTER.parent.parent / FONT_ROOT
FONT_HASHES = {
    'Inter-Regular.ttf': '40d692fce188e4471e2b3cba937be967878f631ad3ebbbdcd587687c7ebe0c82',
    'JetBrainsMono-Regular.ttf': 'a0bf60ef0f83c5ed4d7a75d45838548b1f6873372dfac88f71804491898d138f',
}
FOOTER = 'Public source history and omitted machine-specific evidence are described in [PUBLICATION.md](PUBLICATION.md).'


def git(root, *arguments):
    return subprocess.check_output(['git', '-C', str(root), *arguments], stderr=subprocess.DEVNULL)


class PublicExportTests(unittest.TestCase):
    def fixture(self, root, published, fonts=False, mutate=None):
        (root / 'scripts').mkdir(parents=True)
        shutil.copyfile(EXPORTER, root / 'scripts/export_public_checkpoints.py')
        (root / 'docs').mkdir()
        (root / 'docs/run.txt').write_text('synthetic omitted evidence\n')
        note = ('\n> Public-source note: existing note.\n' if published else '')
        (root / 'docs/REPORT.md').write_text('# Report\n' + note + '\n[log](run.txt)\n')
        (root / 'README.md').write_text('# Example\n' + ('\n' + FOOTER + '\n' if published else ''))
        if published:
            (root / 'PUBLICATION.md').write_text('Obsolete generated publication\n')
        if fonts:
            shutil.copytree(FONT_SOURCE, root / FONT_ROOT)
            shutil.copyfile(EXPORTER.parent.parent / 'THIRD_PARTY_NOTICES.md',
                            root / 'THIRD_PARTY_NOTICES.md')
        if mutate:
            mutate(root)
        git(root, 'init', '-q')
        git(root, 'add', '.')
        git(root, '-c', 'user.name=Export test', '-c', 'user.email=test@localhost',
            'commit', '-qm', 'Synthetic export fixture')
        git(root, 'tag', 'test-stage')

    def run_export(self, source, destination, check=True):
        return subprocess.run([sys.executable, str(source / 'scripts/export_public_checkpoints.py'),
                               '--destination', str(destination), '--tags', 'test-stage'],
                              check=check, capture_output=True, text=True, timeout=30)

    def check_export(self, published, fonts=False):
        with tempfile.TemporaryDirectory(prefix='cedar-export-test-') as directory:
            base = Path(directory)
            source = base / 'source'
            self.fixture(source, published, fonts=fonts)
            destination = base / 'export'
            self.run_export(source, destination)
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
            if fonts:
                files = {item['path']: item for item in stage['files']}
                for original in sorted(FONT_SOURCE.iterdir()):
                    path = (FONT_ROOT / original.name).as_posix()
                    data = original.read_bytes()
                    self.assertEqual((exported / path).read_bytes(), data)
                    self.assertEqual(files[path]['bytes'], len(data))
                    self.assertEqual(files[path]['sha256'], hashlib.sha256(data).hexdigest())
                    if original.name in FONT_HASHES:
                        self.assertEqual(files[path]['sha256'], FONT_HASHES[original.name])
                        self.assertEqual(files[path]['encoding'], 'base64')
                    else:
                        self.assertNotIn('encoding', files[path])
                self.assertEqual((exported / 'THIRD_PARTY_NOTICES.md').read_bytes(),
                                 (source / 'THIRD_PARTY_NOTICES.md').read_bytes())
            git(exported, 'init', '-q')
            git(exported, 'add', '.')
            self.assertEqual(git(exported, 'write-tree').decode().strip(), stage['expected_tree_sha'])

    def test_initial_source_exports_one_publication_and_evidence_note(self):
        self.check_export(False)

    def test_recovered_public_source_does_not_duplicate_paths_or_notes(self):
        self.check_export(True)

    def test_approved_fonts_and_notices_export_byte_identically(self):
        for published in (False, True):
            with self.subTest(published=published):
                self.check_export(published, fonts=True)

    def assert_rejected(self, mutate, message, fonts=True):
        with tempfile.TemporaryDirectory(prefix='cedar-export-reject-') as directory:
            base = Path(directory)
            source = base / 'source'
            self.fixture(source, False, fonts=fonts, mutate=mutate)
            destination = base / 'export'
            result = self.run_export(source, destination, check=False)
            self.assertNotEqual(result.returncode, 0, result.stdout)
            self.assertIn(message, result.stderr)
            self.assertFalse((destination / 'publish-manifest.json').exists())

    def test_tampered_fonts_fail_even_when_manifest_matches_new_bytes(self):
        for name in FONT_HASHES:
            for data in (b'unapproved text replacement\n', b'\0unapproved\xfffont'):
                with self.subTest(name=name, data=data):
                    def mutate(root):
                        (root / FONT_ROOT / name).write_bytes(data)
                        path = root / FONT_ROOT / 'manifest.json'
                        manifest = json.loads(path.read_text())
                        for asset in manifest['assets']:
                            if asset['file'] == name:
                                asset['sha256'] = hashlib.sha256(data).hexdigest()
                                asset['size_bytes'] = len(data)
                        path.write_text(json.dumps(manifest))
                    self.assert_rejected(mutate, 'unapproved font bytes or manifest mismatch')

    def test_unknown_binaries_do_not_gain_a_font_exception(self):
        for name, data in (('other.ttf', b'\xffunapproved'),
                           ('other.bin', b'otherwise-valid-utf8\0binary'),
                           ('renamed.ttf', (FONT_SOURCE / 'Inter-Regular.ttf').read_bytes())):
            with self.subTest(name=name):
                self.assert_rejected(lambda root: (root / name).write_bytes(data),
                                     'UnicodeDecodeError' if name != 'other.bin' else 'unexpected binary')

    def test_approved_hash_at_an_unapproved_path_is_rejected_without_bundle(self):
        self.assert_rejected(
            lambda root: shutil.copyfile(FONT_SOURCE / 'Inter-Regular.ttf', root / 'Inter-Regular.ttf'),
            'UnicodeDecodeError', fonts=False)

    def test_manifest_fields_cannot_change_the_approved_bundle(self):
        for field, value in (('sha256', '0' * 64), ('size_bytes', 1), ('license', 'MIT'),
                             ('license_file', '../unapproved.txt'), ('license_sha256', '0' * 64),
                             ('modified', True)):
            with self.subTest(field=field):
                def mutate(root):
                    path = root / FONT_ROOT / 'manifest.json'
                    manifest = json.loads(path.read_text())
                    manifest['assets'][0][field] = value
                    path.write_text(json.dumps(manifest))
                self.assert_rejected(mutate, 'manifest mismatch')

    def test_manifest_requires_exactly_the_two_approved_entries(self):
        for change in ('duplicate', 'unknown', 'extra', 'missing'):
            with self.subTest(change=change):
                def mutate(root):
                    path = root / FONT_ROOT / 'manifest.json'
                    manifest = json.loads(path.read_text())
                    if change == 'duplicate':
                        manifest['assets'][1] = manifest['assets'][0]
                    elif change == 'unknown':
                        manifest['assets'][0]['file'] = 'other.ttf'
                    elif change == 'extra':
                        manifest['assets'].append(dict(manifest['assets'][0]))
                    else:
                        manifest['assets'].pop()
                    path.write_text(json.dumps(manifest))
                self.assert_rejected(mutate, 'font manifest')

    def test_font_manifest_must_be_readable_valid_json(self):
        for data, message in ((b'\xff', 'UnicodeDecodeError'), (b'{', 'JSONDecodeError'),
                              (b'[]', 'invalid font manifest')):
            with self.subTest(data=data):
                self.assert_rejected(lambda root: (root / FONT_ROOT / 'manifest.json').write_bytes(data),
                                     message)

    def test_complete_tracked_font_bundle_is_required(self):
        for name in ('Inter-Regular.ttf', 'JetBrainsMono-Regular.ttf', 'manifest.json',
                     'Inter-LICENSE.txt', 'JetBrainsMono-OFL.txt', 'README.md'):
            with self.subTest(name=name):
                self.assert_rejected(lambda root: (root / FONT_ROOT / name).unlink(),
                                     'missing or invalid font bundle file')
        self.assert_rejected(lambda root: (root / 'THIRD_PARTY_NOTICES.md').unlink(),
                             'missing or invalid font bundle file')

    def test_exact_license_bytes_and_complete_embedded_notices_are_required(self):
        for name in ('Inter-LICENSE.txt', 'JetBrainsMono-OFL.txt'):
            with self.subTest(name=name):
                self.assert_rejected(lambda root: (root / FONT_ROOT / name).write_text('OFL-1.1\n'),
                                     'unapproved font license')
        self.assert_rejected(lambda root: (root / 'THIRD_PARTY_NOTICES.md').write_text('No font notices.\n'),
                             'incomplete font notices')

    def test_sensitive_text_is_still_blocked_with_approved_fonts(self):
        def mutate(root):
            path = root / FONT_ROOT / 'manifest.json'
            manifest = json.loads(path.read_text())
            manifest['verification'] = 'ghp_' + 'a' * 36
            path.write_text(json.dumps(manifest))
        self.assert_rejected(mutate, 'GitHub token')

    def test_font_validation_reads_tagged_blobs_instead_of_working_tree(self):
        with tempfile.TemporaryDirectory(prefix='cedar-export-tagged-fonts-') as directory:
            base = Path(directory)
            source = base / 'source'
            self.fixture(source, False, fonts=True)
            for original in FONT_SOURCE.iterdir():
                (source / FONT_ROOT / original.name).write_bytes(b'uncommitted corruption')
            (source / 'THIRD_PARTY_NOTICES.md').write_text('uncommitted corruption')
            destination = base / 'export'
            self.run_export(source, destination)
            for original in FONT_SOURCE.iterdir():
                self.assertEqual((destination / 'test-stage' / FONT_ROOT / original.name).read_bytes(),
                                 original.read_bytes())


if __name__ == '__main__':
    unittest.main()
