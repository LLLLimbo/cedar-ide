#!/usr/bin/env python3
"""Regression checks for clean export and recovery from an existing public tree."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

import export_public_checkpoints as exporter
import vendored_egui

EXPORTER = Path(__file__).with_name('export_public_checkpoints.py')
FONT_ROOT = Path('crates/app/assets/fonts')
FONT_SOURCE = EXPORTER.parent.parent / FONT_ROOT
FONT_HASHES = {
    'Inter-Regular.ttf': '40d692fce188e4471e2b3cba937be967878f631ad3ebbbdcd587687c7ebe0c82',
    'JetBrainsMono-Regular.ttf': 'a0bf60ef0f83c5ed4d7a75d45838548b1f6873372dfac88f71804491898d138f',
}
FOOTER = 'Public source history and omitted machine-specific evidence are described in [PUBLICATION.md](PUBLICATION.md).'


def git_environment():
    environment = {key: value for key, value in os.environ.items()
                   if not key.upper().startswith('GIT_')}
    environment['GIT_CONFIG_NOSYSTEM'] = '1'
    environment['GIT_CONFIG_GLOBAL'] = os.devnull
    return environment


def git(root, *arguments):
    return subprocess.check_output(['git', '-C', str(root), *arguments],
                                   stderr=subprocess.DEVNULL, env=git_environment(), timeout=10)


def tagged_blob(root, path):
    return git(root, 'cat-file', 'blob', 'refs/tags/test-stage:' + Path(path).as_posix())


class PublicExportTests(unittest.TestCase):
    def fixture(self, root, published, fonts=False, mutate=None, crlf_checkout=False, vendor=False):
        (root / 'scripts').mkdir(parents=True)
        shutil.copyfile(EXPORTER, root / 'scripts/export_public_checkpoints.py')
        shutil.copyfile(EXPORTER.with_name('vendored_egui.py'), root / 'scripts/vendored_egui.py')
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
        if vendor:
            shutil.copytree(EXPORTER.parent.parent / vendored_egui.ROOT, root / vendored_egui.ROOT)
            shutil.copytree(EXPORTER.parent.parent / 'third-party-licenses/egui-0.31.1',
                            root / 'third-party-licenses/egui-0.31.1')
            shutil.copyfile(EXPORTER.parent.parent / 'THIRD_PARTY_NOTICES.md',
                            root / 'THIRD_PARTY_NOTICES.md')
            shutil.copyfile(EXPORTER.parent.parent / '.gitattributes', root / '.gitattributes')
            (root / 'Cargo.toml').write_text(
                '[patch.crates-io]\negui = { path = "vendor/egui-0.31.1" }\n')
        if crlf_checkout:
            (root / 'checkout-control.txt').write_bytes(b'first\nsecond\n')
        if mutate:
            mutate(root)
        git(root, 'init', '-q')
        # Keep clean/smudge behavior deterministic, including on Linux hosts.
        git(root, 'config', 'core.autocrlf', 'true')
        git(root, 'config', 'core.safecrlf', 'false')
        git(root, 'add', '.')
        git(root, '-c', 'user.name=Export test', '-c', 'user.email=test@localhost',
            'commit', '-qm', 'Synthetic export fixture')
        git(root, 'tag', 'test-stage')
        if crlf_checkout:
            self.assert_crlf_checkout(root)

    def assert_crlf_checkout(self, source):
        text_paths = [FONT_ROOT / name for name in
                      ('Inter-LICENSE.txt', 'JetBrainsMono-OFL.txt', 'README.md', 'manifest.json')]
        text_paths += [Path('THIRD_PARTY_NOTICES.md'), Path('checkout-control.txt')]
        # Removing tracked files forces Git to perform a real checkout conversion.
        for path in text_paths:
            (source / path).unlink()
        git(source, 'checkout-index', '--all')
        for path in text_paths:
            data = tagged_blob(source, path)
            self.assertIn(b'\n', data, str(path))
            self.assertNotIn(b'\r\n', data, str(path))
            checkout = (source / path).read_bytes()
            self.assertEqual(checkout, data.replace(b'\n', b'\r\n'), str(path))
            self.assertNotEqual(checkout, data, str(path))
        self.assertEqual((source / 'checkout-control.txt').read_bytes(), b'first\r\nsecond\r\n')
        for name, sha256 in FONT_HASHES.items():
            data = (source / FONT_ROOT / name).read_bytes()
            self.assertEqual(data, tagged_blob(source, FONT_ROOT / name))
            self.assertEqual(hashlib.sha256(data).hexdigest(), sha256)

    def assert_font_export(self, source, stage):
        exported = Path(stage['directory'])
        files = {item['path']: item for item in stage['files']}
        paths = [FONT_ROOT / original.name for original in sorted(FONT_SOURCE.iterdir())]
        paths.append(Path('THIRD_PARTY_NOTICES.md'))
        for relative in paths:
            path = relative.as_posix()
            # The immutable tagged blob is the byte oracle, never the checkout.
            data = tagged_blob(source, path)
            self.assertEqual((exported / path).read_bytes(), data, path)
            self.assertEqual(files[path]['bytes'], len(data), path)
            self.assertEqual(files[path]['sha256'], hashlib.sha256(data).hexdigest(), path)
            if relative.name in FONT_HASHES:
                self.assertEqual(files[path]['sha256'], FONT_HASHES[relative.name])
                self.assertEqual(files[path]['encoding'], 'base64')
            else:
                self.assertNotIn('encoding', files[path])

    def run_export(self, source, destination, check=True):
        return subprocess.run([sys.executable, str(source / 'scripts/export_public_checkpoints.py'),
                               '--destination', str(destination), '--tags', 'test-stage'],
                              check=check, capture_output=True, text=True, timeout=30,
                              env=git_environment())

    def check_export(self, published, fonts=False, crlf_checkout=False):
        with tempfile.TemporaryDirectory(prefix='cedar-export-test-') as directory:
            base = Path(directory)
            source = base / 'source'
            self.fixture(source, published, fonts=fonts, crlf_checkout=crlf_checkout)
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
                self.assert_font_export(source, stage)
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

    def test_autocrlf_checkout_exports_exact_tagged_fonts_and_notices(self):
        for published in (False, True):
            with self.subTest(published=published):
                self.check_export(published, fonts=True, crlf_checkout=True)

    def test_crlf_license_blobs_are_rejected_without_normalizing(self):
        for name in ('Inter-LICENSE.txt', 'JetBrainsMono-OFL.txt'):
            with self.subTest(name=name):
                with tempfile.TemporaryDirectory(prefix='cedar-export-crlf-license-') as directory:
                    base = Path(directory)
                    source = base / 'source'
                    path = FONT_ROOT / name
                    # Deliberately create wrong license bytes, even if the host checkout is CRLF.
                    data = (FONT_SOURCE / name).read_text(encoding='utf-8').replace('\n', '\r\n').encode('utf-8')

                    def mutate(root):
                        (root / path).write_bytes(data)
                        # Preserve this corruption in the tag despite core.autocrlf=true.
                        (root / '.gitattributes').write_bytes((path.as_posix() + ' -text\n').encode())

                    self.fixture(source, False, fonts=True, mutate=mutate)
                    self.assertIn(b'\r\n', tagged_blob(source, path))
                    self.assertEqual(tagged_blob(source, path), data)
                    destination = base / 'export'
                    result = self.run_export(source, destination, check=False)
                    self.assertNotEqual(result.returncode, 0, result.stdout)
                    self.assertIn('unapproved font license in ' + name, result.stderr)
                    self.assertFalse((destination / 'publish-manifest.json').exists())

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
            self.fixture(source, False, fonts=True, crlf_checkout=True)
            for original in FONT_SOURCE.iterdir():
                (source / FONT_ROOT / original.name).write_bytes(b'uncommitted corruption')
            (source / 'THIRD_PARTY_NOTICES.md').write_text('uncommitted corruption')
            destination = base / 'export'
            self.run_export(source, destination)
            stage = json.loads((destination / 'publish-manifest.json').read_text())['stages'][0]
            self.assert_font_export(source, stage)


    def assert_vendor_export(self, source, stage):
        exported = Path(stage['directory'])
        files = {item['path']: item for item in stage['files']}
        paths = [path.relative_to(source).as_posix()
                 for path in (source / vendored_egui.ROOT).rglob('*') if path.is_file()]
        paths += ['third-party-licenses/egui-0.31.1/' + name
                  for name in vendored_egui.LICENSE_HASHES]
        self.assertEqual(len(paths), 100)  # 92 upstream + six additions + two bundle copies.
        for path in paths:
            data = tagged_blob(source, path)
            self.assertEqual((exported / path).read_bytes(), data, path)
            self.assertEqual(files[path]['sha256'], hashlib.sha256(data).hexdigest(), path)
            self.assertNotIn('encoding', files[path])  # No new binary transport exception.
        self.assertEqual((exported / 'THIRD_PARTY_NOTICES.md').read_text().count('| egui | 0.31.1 |'), 1)

    def test_complete_vendor_source_patch_and_provenance_export_exact_tagged_bytes(self):
        for published, fonts in ((False, False), (True, True)):
            with self.subTest(published=published, fonts=fonts), tempfile.TemporaryDirectory(prefix='cedar-export-vendor-') as directory:
                base = Path(directory)
                source = base / 'source'
                self.fixture(source, published, fonts=fonts, vendor=True)
                destination = base / 'export'
                self.run_export(source, destination)
                stage = json.loads((destination / 'publish-manifest.json').read_text())['stages'][0]
                self.assert_vendor_export(source, stage)
                if fonts:
                    self.assert_font_export(source, stage)
                git(Path(stage['directory']), 'init', '-q')
                git(Path(stage['directory']), 'add', '.')
                self.assertEqual(git(Path(stage['directory']), 'write-tree').decode().strip(), stage['expected_tree_sha'])

    def test_vendor_autocrlf_checkout_and_uncommitted_changes_cannot_change_tagged_export(self):
        with tempfile.TemporaryDirectory(prefix='cedar-vendor-checkout-') as directory:
            base = Path(directory)
            source = base / 'source'
            self.fixture(source, False, vendor=True)
            paths = [path.relative_to(source).as_posix()
                     for path in (source / vendored_egui.ROOT).rglob('*') if path.is_file()]
            paths += ['third-party-licenses/egui-0.31.1/' + name for name in vendored_egui.LICENSE_HASHES]
            for path in paths:
                (source / path).unlink()
            git(source, 'checkout-index', '--all')
            for path in paths:
                self.assertEqual((source / path).read_bytes(), tagged_blob(source, path), path)
            # Checkouts are never the source byte oracle for immutable-tag export.
            (source / vendored_egui.ROOT / 'README.md').write_text('uncommitted change\n')
            destination = base / 'export'
            self.run_export(source, destination)
            stage = json.loads((destination / 'publish-manifest.json').read_text())['stages'][0]
            self.assert_vendor_export(source, stage)

    def test_vendor_rejects_missing_changed_extra_or_unapproved_files(self):
        cases = [
            ('source', 'unapproved bytes'), ('license', 'unapproved bytes'),
            ('provenance', 'unapproved bytes'), ('patch', 'unapproved bytes'),
            ('extra-binary', 'unexpected or missing source file set'),
            ('missing-source', 'unexpected or missing source file set'),
            ('missing-license', 'missing approved file'),
            ('missing-bundle-license', 'missing or invalid vendor bundle file'),
            ('wrong-bundle-license', 'missing or unapproved bundled license'),
            ('wrong-patch', 'unapproved Cargo patch identity/path'),
            ('missing-patch', 'vendor source has no exact Cargo patch'),
            ('duplicate-notice', 'missing or duplicate dependency/modification notice'),
        ]
        for change, message in cases:
            with self.subTest(change=change), tempfile.TemporaryDirectory(prefix='cedar-vendor-export-reject-') as directory:
                base = Path(directory)
                source = base / 'source'
                def mutate(root):
                    vendor = root / vendored_egui.ROOT
                    if change in ('source', 'license', 'provenance', 'patch'):
                        name = {'source': 'src/lib.rs', 'license': 'LICENSE-MIT',
                                'provenance': 'vendor-upstream.json', 'patch': 'egui-observation.patch'}[change]
                        with (vendor / name).open('ab') as output:
                            output.write(b' ')
                    elif change == 'extra-binary':
                        (vendor / 'extra.bin').write_bytes(b'\x00\xff')
                    elif change == 'missing-source':
                        (vendor / 'src/lib.rs').unlink()
                    elif change == 'missing-license':
                        (vendor / 'LICENSE-MIT').unlink()
                    elif change == 'missing-bundle-license':
                        (root / 'third-party-licenses/egui-0.31.1/LICENSE-MIT').unlink()
                    elif change == 'wrong-bundle-license':
                        (root / 'third-party-licenses/egui-0.31.1/LICENSE-MIT').write_bytes(b'wrong license')
                    elif change == 'wrong-patch':
                        (root / 'Cargo.toml').write_text('[patch.crates-io]\negui = { path = "elsewhere" }\n')
                    elif change == 'missing-patch':
                        (root / 'Cargo.toml').unlink()
                    else:
                        with (root / 'THIRD_PARTY_NOTICES.md').open('a') as output:
                            output.write('| egui | 0.31.1 | MIT OR Apache-2.0 | https://github.com/emilk/egui |\n')
                self.fixture(source, False, vendor=True, mutate=mutate)
                destination = base / 'export'
                result = self.run_export(source, destination, check=False)
                self.assertNotEqual(result.returncode, 0, result.stdout)
                self.assertIn(message, result.stderr)
                self.assertFalse((destination / 'publish-manifest.json').exists())

    def test_verified_vendor_bytes_still_pass_the_global_sensitive_scan(self):
        with tempfile.TemporaryDirectory(prefix='cedar-vendor-sensitive-') as directory:
            base = Path(directory)
            source = base / 'source'
            self.fixture(source, False, vendor=True)
            destination = base / 'export'
            with mock.patch.object(exporter, '__file__', str(source / 'scripts/export_public_checkpoints.py')):
                with mock.patch.object(sys, 'argv', ['export', '--destination', str(destination), '--tags', 'test-stage']):
                    with mock.patch.dict(exporter.SENSITIVE_PATTERNS, {'vendor canary': rb'Cedar receipt prototype'}):
                        with self.assertRaisesRegex(RuntimeError, 'vendor canary in vendor/egui-0.31.1/'):
                            exporter.main()
            self.assertFalse((destination / 'publish-manifest.json').exists())


if __name__ == '__main__':
    unittest.main()
