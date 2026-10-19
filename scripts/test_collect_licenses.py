#!/usr/bin/env python3
"""Notice regeneration must retain non-Cargo fixtures and approved font licenses."""
import contextlib
import hashlib
import io
import json
from pathlib import Path
import runpy
import shutil
import tempfile
import tomllib
import unittest
from unittest import mock

import collect_licenses as collector
import vendored_egui

ROOT = Path(__file__).resolve().parent.parent


class UnicodeNoticeCollectionTests(unittest.TestCase):
    def fixture(self, root):
        (root / 'Cargo.toml').write_text('[workspace]\nmembers = []\n')
        fonts = root / 'crates/app/assets/fonts'
        fonts.mkdir(parents=True)
        for name in ('manifest.json', 'Inter-LICENSE.txt', 'JetBrainsMono-OFL.txt'):
            shutil.copyfile(ROOT / 'crates/app/assets/fonts' / name, fonts / name)
        base = root / 'registry/regex-syntax-0.8.11'
        base.mkdir(parents=True)
        (base / 'Cargo.toml').write_text('[package]\nname = "regex-syntax"\nversion = "0.8.11"\n')
        (base / 'LICENSE-MIT').write_bytes(b'Synthetic crate MIT notice\n')
        (base / 'LICENSE-APACHE').write_bytes(b'Synthetic crate Apache notice\n')
        notice = base / collector.UNICODE_NOTICE_PATH
        notice.parent.mkdir(parents=True)
        content = (ROOT / 'third-party-licenses/regex-syntax-0.8.11/LICENSE-UNICODE').read_bytes()
        self.assertEqual(len(content), 2847)
        self.assertEqual(hashlib.sha256(content).hexdigest(),
                         '74db5baf44a41b1000312c673544b3374e4198af5605c7f9080a402cec42cfa3')
        notice.write_bytes(content)
        package = {'name': 'regex-syntax', 'version': '0.8.11',
                   'source': 'registry+https://github.com/rust-lang/crates.io-index',
                   'manifest_path': str(base / 'Cargo.toml'),
                   'license': 'MIT OR Apache-2.0', 'license_file': None,
                   'repository': 'https://github.com/rust-lang/regex'}
        return {'workspace_members': [], 'packages': [package]}, base, notice

    def test_exact_nested_notice_is_preserved_once_and_regeneration_is_idempotent(self):
        with tempfile.TemporaryDirectory(prefix='cedar-unicode-notices-') as directory:
            root = Path(directory).resolve()
            metadata, base, notice = self.fixture(root)
            # The rule must not recursively collect any other nested notice or source.
            (notice.parent / 'LICENSE-UNREVIEWED').write_text('Must not be copied\n')
            (notice.parent / 'case_folding_simple.rs').write_text('// Synthetic source\n')
            before_source = {path.relative_to(base).as_posix(): path.read_bytes()
                             for path in base.rglob('*') if path.is_file()}
            previous = None
            for _ in range(2):
                self.assertEqual(collector.collect(root, metadata), 1)
                out = root / 'third-party-licenses'
                actual = {path.relative_to(out).as_posix(): path.read_bytes()
                          for path in out.rglob('*') if path.is_file()}
                self.assertEqual(set(actual), {'regex-syntax-0.8.11/LICENSE-MIT',
                                               'regex-syntax-0.8.11/LICENSE-APACHE',
                                               'regex-syntax-0.8.11/LICENSE-UNICODE'})
                self.assertEqual(actual['regex-syntax-0.8.11/LICENSE-UNICODE'], notice.read_bytes())
                notices = (root / 'THIRD_PARTY_NOTICES.md').read_bytes()
                text = notices.decode()
                self.assertEqual(text.count('| regex-syntax | 0.8.11 |'), 1)
                self.assertEqual(text.count('## Unicode-derived data'), 1)
                self.assertIn('Unicode 16.0.0 simple case-folding table', text)
                self.assertIn('third-party-licenses/regex-syntax-0.8.11/LICENSE-UNICODE', text)
                self.assertEqual(text.count('## Apache Maven Surefire report fixture'), 1)
                self.assertEqual(text.count('## Embedded fonts'), 1)
                if previous is not None:
                    self.assertEqual((notices, actual), previous)
                previous = notices, actual
            self.assertEqual({path.relative_to(base).as_posix(): path.read_bytes()
                              for path in base.rglob('*') if path.is_file()}, before_source)

    def test_unreviewed_package_identity_is_rejected_before_any_output(self):
        for change in ('version', 'source', 'repository', 'license', 'license_file', 'duplicate'):
            with self.subTest(change=change), tempfile.TemporaryDirectory(prefix='cedar-unicode-identity-') as directory:
                root = Path(directory).resolve()
                metadata, _, _ = self.fixture(root)
                package = metadata['packages'][0]
                if change == 'duplicate':
                    metadata['packages'].append(dict(package))
                elif change == 'license_file':
                    package[change] = '../elsewhere/LICENSE-UNICODE'
                else:
                    package[change] = 'unreviewed'
                with self.assertRaisesRegex(ValueError, 'regex-syntax Unicode notice:'):
                    collector.collect(root, metadata)
                self.assertFalse((root / 'third-party-licenses').exists())
                self.assertFalse((root / 'THIRD_PARTY_NOTICES.md').exists())

    def test_missing_unexpected_and_modified_notice_paths_fail_closed(self):
        for change in ('missing', 'root-only', 'elsewhere-only', 'root-shadow', 'modified-bytes', 'modified-size',
                       'manifest-name', 'manifest-missing', 'package-directory', 'notice-directory'):
            with self.subTest(change=change), tempfile.TemporaryDirectory(prefix='cedar-unicode-path-') as directory:
                root = Path(directory).resolve()
                metadata, base, notice = self.fixture(root)
                package = metadata['packages'][0]
                if change == 'missing':
                    notice.unlink()
                elif change == 'root-only':
                    notice.rename(base / notice.name)
                elif change == 'elsewhere-only':
                    notice.rename(base / 'src' / notice.name)
                elif change == 'root-shadow':
                    shutil.copyfile(notice, base / notice.name)
                elif change == 'modified-bytes':
                    notice.write_bytes(b'X' + notice.read_bytes()[1:])
                elif change == 'modified-size':
                    notice.write_bytes(notice.read_bytes() + b' ')
                elif change == 'manifest-name':
                    renamed = base / 'Unexpected.toml'
                    (base / 'Cargo.toml').rename(renamed)
                    package['manifest_path'] = str(renamed)
                elif change == 'manifest-missing':
                    (base / 'Cargo.toml').unlink()
                elif change == 'package-directory':
                    renamed = base.with_name('unreviewed-package')
                    base.rename(renamed)
                    package['manifest_path'] = str(renamed / 'Cargo.toml')
                else:
                    notice.unlink()
                    notice.mkdir()
                with self.assertRaisesRegex(ValueError, 'regex-syntax Unicode notice:'):
                    collector.collect(root, metadata)
                self.assertFalse((root / 'third-party-licenses').exists())
                self.assertFalse((root / 'THIRD_PARTY_NOTICES.md').exists())

    def test_missing_source_cannot_be_masked_by_a_previously_collected_notice(self):
        with tempfile.TemporaryDirectory(prefix='cedar-unicode-stale-') as directory:
            root = Path(directory).resolve()
            metadata, _, notice = self.fixture(root)
            collector.collect(root, metadata)
            out = root / 'third-party-licenses'
            before = {path.relative_to(out).as_posix(): path.read_bytes()
                      for path in out.rglob('*') if path.is_file()}
            before_notices = (root / 'THIRD_PARTY_NOTICES.md').read_bytes()
            notice.unlink()
            with self.assertRaisesRegex(ValueError, 'missing, unsafe or unexpected nested notice path'):
                collector.collect(root, metadata)
            self.assertEqual({path.relative_to(out).as_posix(): path.read_bytes()
                              for path in out.rglob('*') if path.is_file()}, before)
            self.assertEqual((root / 'THIRD_PARTY_NOTICES.md').read_bytes(), before_notices)

    def test_symlinked_source_and_destination_paths_are_rejected(self):
        for change in ('manifest', 'notice', 'notice-parent', 'package-parent',
                       'destination', 'destination-parent'):
            with self.subTest(change=change), tempfile.TemporaryDirectory(prefix='cedar-unicode-symlink-') as directory:
                root = Path(directory).resolve()
                metadata, base, notice = self.fixture(root)
                if change.startswith('destination'):
                    original = root / 'third-party-licenses/regex-syntax-0.8.11/LICENSE-UNICODE'
                    original.parent.mkdir(parents=True)
                    original.write_bytes(b'Existing content must stay unchanged\n')
                    if change == 'destination-parent':
                        original = original.parent
                else:
                    original = {'manifest': base / 'Cargo.toml', 'notice': notice,
                                'notice-parent': notice.parent, 'package-parent': base}[change]
                target = root / 'linked-content'
                original.rename(target)
                try:
                    original.symlink_to(target, target_is_directory=target.is_dir())
                except OSError as error:
                    self.skipTest('Symlink creation unavailable: ' + str(error))
                before = {path.relative_to(target).as_posix(): path.read_bytes()
                          for path in target.rglob('*') if path.is_file()} if target.is_dir() else target.read_bytes()
                with self.assertRaisesRegex(ValueError, 'regex-syntax Unicode notice:'):
                    collector.collect(root, metadata)
                after = {path.relative_to(target).as_posix(): path.read_bytes()
                         for path in target.rglob('*') if path.is_file()} if target.is_dir() else target.read_bytes()
                self.assertEqual(after, before)
                self.assertFalse((root / 'THIRD_PARTY_NOTICES.md').exists())


class LicenseCollectionTests(unittest.TestCase):
    def test_regeneration_keeps_dependency_fixture_and_font_notices(self):
        self.regeneration_case(alias_entry=False)

    def test_noncanonical_script_entry_keeps_exact_canonical_cargo_root(self):
        self.regeneration_case(alias_entry=True)

    def regeneration_case(self, alias_entry):
        with tempfile.TemporaryDirectory(prefix='cedar-notice-test-') as directory:
            root = Path(directory)
            expected_root = root.resolve()
            if alias_entry:
                (root / 'route').mkdir()
                root = root / 'route' / '..'
                self.assertNotEqual(root, expected_root)
                self.assertEqual(root.resolve(), expected_root)
            (root / 'scripts').mkdir()
            script = root / 'scripts/collect_licenses.py'
            shutil.copyfile(ROOT / 'scripts/collect_licenses.py', script)
            shutil.copyfile(ROOT / 'scripts/vendored_egui.py', root / 'scripts/vendored_egui.py')
            (root / 'Cargo.toml').write_text('[workspace]\nmembers = ["crates/example"]\n')
            (root / 'crates/example').mkdir(parents=True)
            (root / 'crates/example/Cargo.toml').write_text('[package]\nname = "cedar-example"\nversion = "0.71.0"\n')
            fonts = root / 'crates/app/assets/fonts'
            fonts.mkdir(parents=True)
            for name in ['manifest.json', 'Inter-LICENSE.txt', 'JetBrainsMono-OFL.txt']:
                shutil.copyfile(ROOT / 'crates/app/assets/fonts' / name, fonts / name)
            font_bytes = {path.name: path.read_bytes() for path in fonts.iterdir()}
            dependency = root / 'dependency'
            dependency.mkdir()
            (dependency / 'LICENSE').write_text('Synthetic dependency license\n')
            metadata = {'workspace_members': ['cedar-example'], 'packages': [
                {'id': 'cedar-example', 'name': 'cedar-example', 'version': '0.71.0', 'source': None,
                 'manifest_path': str(expected_root / 'crates/example/Cargo.toml')},
                {'name': 'synthetic-dependency', 'version': '1.0', 'source': 'registry',
                 'manifest_path': str(dependency / 'Cargo.toml'), 'license': 'MIT',
                 'repository': 'https://example.invalid/dependency'},
            ]}
            previous = None
            previous_bytes = None
            for _ in range(2):
                stdout = io.StringIO()
                with mock.patch('subprocess.check_output', return_value=json.dumps(metadata).encode()) as cargo:
                    with contextlib.redirect_stdout(stdout):
                        runpy.run_path(str(script), run_name='__main__')
                cargo.assert_called_once_with(
                    ['cargo', 'metadata', '--format-version', '1', '--locked', '--offline'], cwd=expected_root)
                self.assertIn('Collected notices for 1 locked packages', stdout.getvalue())
                notice_bytes = (root / 'THIRD_PARTY_NOTICES.md').read_bytes()
                text = (root / 'THIRD_PARTY_NOTICES.md').read_text()
                self.assertIn('| synthetic-dependency | 1.0 | MIT |', text)
                self.assertEqual(text.count('## Apache Maven Surefire report fixture'), 1)
                self.assertIn('third-party-licenses/apache-maven-surefire-3.5.4/LICENSE', text)
                self.assertIn('Only text fixture data is included; no Surefire/JUnit executable is redistributed.', text)
                for name in ['Inter-LICENSE.txt', 'JetBrainsMono-OFL.txt']:
                    license_text = '\n'.join(line.rstrip() for line in (fonts / name).read_text().splitlines()).strip()
                    self.assertIn(license_text, text)
                if previous is not None:
                    self.assertEqual(previous, text)
                    self.assertEqual(previous_bytes, notice_bytes)
                previous = text
                previous_bytes = notice_bytes
                self.assertEqual({path.name: path.read_bytes() for path in fonts.iterdir()}, font_bytes)
                self.assertEqual((root / 'third-party-licenses/synthetic-dependency-1.0/LICENSE').read_bytes(),
                                 (dependency / 'LICENSE').read_bytes())
                self.assertEqual((root / 'third-party-licenses/synthetic-dependency-1.0/LICENSE').read_text(),
                                 'Synthetic dependency license\n')


    def vendor_fixture(self, root):
        root = root.resolve()
        shutil.copytree(ROOT / vendored_egui.ROOT, root / vendored_egui.ROOT)
        (root / 'Cargo.toml').write_text(
            '[workspace]\nmembers = ["crates/example"]\n'
            '[patch.crates-io]\negui = { path = "vendor/egui-0.31.1" }\n')
        member = root / 'crates/example/Cargo.toml'
        member.parent.mkdir(parents=True)
        member.write_text('[package]\nname = "cedar-example"\nversion = "1.0.0"\n')
        fonts = root / 'crates/app/assets/fonts'
        fonts.mkdir(parents=True)
        for name in ('manifest.json', 'Inter-LICENSE.txt', 'JetBrainsMono-OFL.txt'):
            shutil.copyfile(ROOT / 'crates/app/assets/fonts' / name, fonts / name)
        packages = [
            {'id': 'workspace-example', 'name': 'cedar-example', 'version': '1.0.0',
             'source': None, 'manifest_path': str(member)},
            {'id': 'local-egui', 'name': 'egui', 'version': '0.31.1', 'source': None,
             'manifest_path': str(root / vendored_egui.ROOT / 'Cargo.toml'),
             'license': 'MIT OR Apache-2.0', 'repository': 'https://github.com/emilk/egui'},
        ]
        for platform in ('linux', 'windows'):
            registry = root / ('registry-' + platform)
            registry.mkdir()
            (registry / 'LICENSE').write_text(platform + ' unchanged upstream notice\n')
            packages.append({'name': platform + '-dependency', 'version': '1.0.0',
                             'source': 'registry', 'manifest_path': str(registry / 'Cargo.toml'),
                             'license': 'MIT', 'repository': 'https://example.invalid/' + platform})
        return {'workspace_members': ['workspace-example'], 'packages': packages}

    def test_one_egui_contribution_preserves_both_platform_dependencies_and_is_idempotent(self):
        with tempfile.TemporaryDirectory(prefix='cedar-vendor-notices-') as directory:
            root = Path(directory).resolve()
            metadata = self.vendor_fixture(root)
            self.assertEqual(collector.collect(root, metadata), 3)
            out = root / 'third-party-licenses'
            before = {path.relative_to(out).as_posix(): path.read_bytes()
                      for path in out.rglob('*') if path.is_file()}
            notices = (root / 'THIRD_PARTY_NOTICES.md').read_bytes()
            self.assertEqual(collector.collect(root, metadata), 3)
            self.assertEqual((root / 'THIRD_PARTY_NOTICES.md').read_bytes(), notices)
            self.assertEqual({path.relative_to(out).as_posix(): path.read_bytes()
                              for path in out.rglob('*') if path.is_file()}, before)
            text = notices.decode()
            self.assertEqual(text.count('| egui | 0.31.1 |'), 1)
            self.assertEqual(text.count('## Vendored egui 0.31.1'), 1)
            self.assertNotIn('No third-party source code', text)
            self.assertEqual(set(before), {'egui-0.31.1/LICENSE-MIT', 'egui-0.31.1/LICENSE-APACHE',
                                           'linux-dependency-1.0.0/LICENSE', 'windows-dependency-1.0.0/LICENSE'})
            for name in vendored_egui.LICENSE_HASHES:
                self.assertEqual(before['egui-0.31.1/' + name],
                                 (root / vendored_egui.ROOT / name).read_bytes())
            for platform in ('linux', 'windows'):
                self.assertEqual(before[platform + '-dependency-1.0.0/LICENSE'],
                                 (root / ('registry-' + platform) / 'LICENSE').read_bytes())
                self.assertIn('| ' + platform + '-dependency | 1.0.0 | MIT |', text)

    def test_registry_to_vendor_keeps_the_same_logical_package_rows(self):
        with tempfile.TemporaryDirectory(prefix='cedar-vendor-identity-') as directory:
            root = Path(directory).resolve()
            metadata = self.vendor_fixture(root)
            patched_manifest = (root / 'Cargo.toml').read_bytes()
            package = metadata['packages'][1]
            package['source'] = 'registry'
            registry = root / 'registry-egui'
            registry.mkdir()
            for name in vendored_egui.LICENSE_HASHES:
                shutil.copyfile(root / vendored_egui.ROOT / name, registry / name)
            package['manifest_path'] = str(registry / 'Cargo.toml')
            (root / 'Cargo.toml').write_text('[workspace]\nmembers = ["crates/example"]\n')
            self.assertEqual(collector.collect(root, metadata), 3)
            rows = [line for line in (root / 'THIRD_PARTY_NOTICES.md').read_text().splitlines()
                    if line.startswith('|')]
            package['source'] = None
            package['manifest_path'] = str(root / vendored_egui.ROOT / 'Cargo.toml')
            (root / 'Cargo.toml').write_bytes(patched_manifest)
            self.assertEqual(collector.collect(root, metadata), 3)
            self.assertEqual([line for line in (root / 'THIRD_PARTY_NOTICES.md').read_text().splitlines()
                              if line.startswith('|')], rows)

    def test_wrong_local_identity_unknown_paths_and_duplicate_egui_fail_closed(self):
        for change in ('version', 'name', 'path', 'license', 'repository', 'license_file',
                       'unknown-local', 'duplicate', 'registry-duplicate', 'missing'):
            with self.subTest(change=change), tempfile.TemporaryDirectory(prefix='cedar-vendor-reject-') as directory:
                root = Path(directory).resolve()
                metadata = self.vendor_fixture(root)
                package = metadata['packages'][1]
                if change in ('version', 'name', 'license', 'repository'):
                    package[change] = 'unapproved'
                elif change == 'path':
                    package['manifest_path'] = str(root / 'elsewhere/Cargo.toml')
                elif change == 'license_file':
                    package['license_file'] = '../../private.txt'
                elif change == 'unknown-local':
                    metadata['packages'][2]['source'] = None
                elif change in ('duplicate', 'registry-duplicate'):
                    duplicate = dict(package)
                    if change == 'registry-duplicate':
                        duplicate['source'] = 'registry'
                    metadata['packages'].append(duplicate)
                else:
                    metadata['packages'].remove(package)
                with self.assertRaises((ValueError, FileNotFoundError)):
                    collector.collect(root, metadata)
                self.assertFalse((root / 'THIRD_PARTY_NOTICES.md').exists())

    def test_unapproved_vendor_file_bytes_sets_and_symlinks_are_rejected(self):
        for change in ('extra', 'missing-license', 'license-bytes', 'source-bytes',
                       'provenance-bytes', 'symlink-file', 'symlink-directory'):
            with self.subTest(change=change), tempfile.TemporaryDirectory(prefix='cedar-vendor-tree-') as directory:
                root = Path(directory).resolve()
                metadata = self.vendor_fixture(root)
                vendor = root / vendored_egui.ROOT
                if change == 'extra':
                    (vendor / 'extra.rs').write_text('// unreviewed source\n')
                elif change == 'missing-license':
                    (vendor / 'LICENSE-MIT').unlink()
                elif change in ('license-bytes', 'source-bytes', 'provenance-bytes'):
                    name = {'license-bytes': 'LICENSE-MIT', 'source-bytes': 'src/lib.rs',
                            'provenance-bytes': 'vendor-upstream.json'}[change]
                    with (vendor / name).open('ab') as handle:
                        handle.write(b' ')
                else:
                    original = vendor / ('LICENSE-MIT' if change == 'symlink-file' else 'src')
                    target = root / 'linked-content'
                    original.rename(target)
                    try:
                        original.symlink_to(target, target_is_directory=target.is_dir())
                    except OSError as error:
                        self.skipTest('Symlink creation unavailable: ' + str(error))
                with self.assertRaises(ValueError):
                    collector.collect(root, metadata)
                self.assertFalse((root / 'THIRD_PARTY_NOTICES.md').exists())

    def test_committed_notice_inventory_matches_locked_linux_and_windows_graph(self):
        locked = tomllib.loads((ROOT / 'Cargo.lock').read_text())['package']
        expected = {(item['name'], item['version']) for item in locked
                    if 'source' in item or (item['name'], item['version']) == ('egui', '0.31.1')}
        rows = [line.split('|') for line in (ROOT / 'THIRD_PARTY_NOTICES.md').read_text().splitlines()
                if line.startswith('| ') and not line.startswith('| Package |')]
        actual = [(row[1].strip(), row[2].strip()) for row in rows]
        self.assertEqual(len(actual), len(set(actual)))
        self.assertEqual(set(actual), expected)
        self.assertEqual(actual.count(('egui', '0.31.1')), 1)
        self.assertTrue(any(name.startswith('windows') for name, _ in actual))
        self.assertTrue(any(name.startswith('wayland') for name, _ in actual))



    def test_workspace_members_require_declared_paths_identity_and_unique_manifests(self):
        for change in ('undeclared', 'symlink-parent', 'duplicate-manifest', 'name', 'version'):
            with self.subTest(change=change), tempfile.TemporaryDirectory(prefix='cedar-workspace-notices-') as directory:
                root = Path(directory).resolve()
                metadata = self.vendor_fixture(root)
                package = metadata['packages'][0]
                if change == 'undeclared':
                    manifest = root / 'crates/undeclared/Cargo.toml'
                    manifest.parent.mkdir()
                    manifest.write_bytes(Path(package['manifest_path']).read_bytes())
                    package['manifest_path'] = str(manifest)
                    # Cargo's reported member ID alone must not exempt an undeclared path.
                    self.assertIn(package['id'], metadata['workspace_members'])
                elif change == 'symlink-parent':
                    parent = Path(package['manifest_path']).parent
                    target = parent.with_name('actual-example')
                    parent.rename(target)
                    try:
                        parent.symlink_to(target, target_is_directory=True)
                    except OSError as error:
                        self.skipTest('Symlink creation unavailable: ' + str(error))
                elif change == 'duplicate-manifest':
                    duplicate = dict(package, id='another-workspace-id')
                    metadata['workspace_members'].append(duplicate['id'])
                    metadata['packages'].append(duplicate)
                else:
                    package[change] = 'mismatched'
                with self.assertRaises(ValueError):
                    collector.collect(root, metadata)
                self.assertFalse((root / 'THIRD_PARTY_NOTICES.md').exists())

    def test_workspace_version_inheritance_keeps_expected_first_party_members(self):
        with tempfile.TemporaryDirectory(prefix='cedar-workspace-inherited-') as directory:
            root = Path(directory).resolve()
            metadata = self.vendor_fixture(root)
            with (root / 'Cargo.toml').open('a') as output:
                output.write('[workspace.package]\nversion = "1.0.0"\n')
            Path(metadata['packages'][0]['manifest_path']).write_text(
                '[package]\nname = "cedar-example"\nversion.workspace = true\n')
            self.assertEqual(collector.collect(root, metadata), 3)
            self.assertNotIn('| cedar-example |', (root / 'THIRD_PARTY_NOTICES.md').read_text())


class LicenseMaterialSafetyRegressionTests(unittest.TestCase):
    """The 673a collector admitted these invalid synthetic material generations."""

    def material_fixture(self, root):
        (root / 'Cargo.toml').write_text('[workspace]\nmembers = []\n')
        fonts = root / 'crates/app/assets/fonts'
        fonts.mkdir(parents=True)
        (fonts / 'manifest.json').write_text('{"assets":[]}\n')
        base = root / 'registry/example-1.0.0'
        base.mkdir(parents=True)
        (base / 'Cargo.toml').write_text(
            '[package]\nname="example"\nversion="1.0.0"\n')
        (base / 'LICENSE').write_bytes(b'Synthetic original license\n')
        package = {'id': 'registry+example#example@1.0.0',
                   'name': 'example', 'version': '1.0.0',
                   'source': 'registry+https://example.invalid/index',
                   'manifest_path': str(base / 'Cargo.toml'), 'license': 'MIT',
                   'license_file': None, 'repository': 'https://example.invalid/source'}
        metadata = {'workspace_members': [], 'packages': [package]}
        self.assertEqual(collector.collect(root, metadata), 1)
        self.assertEqual((root / 'third-party-licenses/example-1.0.0/LICENSE').read_bytes(),
                         b'Synthetic original license\n')
        return base, metadata

    def material_outputs(self, root):
        outputs = {}
        for name in ('THIRD_PARTY_NOTICES.md', 'LICENSE-MATERIAL-COVERAGE.json',
                     'LICENSE-MATERIALS-README.md'):
            path = root / name
            if path.is_file():
                outputs[name] = path.read_bytes()
        notice_root = root / 'third-party-licenses'
        if notice_root.exists():
            for path in notice_root.rglob('*'):
                if path.is_symlink():
                    outputs[path.relative_to(root).as_posix()] = ('link', str(path.readlink()))
                elif path.is_file():
                    outputs[path.relative_to(root).as_posix()] = path.read_bytes()
        return outputs

    def reject_unchanged(self, root, metadata):
        before = self.material_outputs(root)
        with self.assertRaises(ValueError):
            collector.collect(root, metadata)
        self.assertEqual(self.material_outputs(root), before)

    def test_missing_material_cannot_be_hidden_by_prior_output(self):
        with tempfile.TemporaryDirectory(prefix='cedar-license-missing-') as directory:
            root = Path(directory).resolve()
            base, metadata = self.material_fixture(root)
            (base / 'LICENSE').unlink()
            self.reject_unchanged(root, metadata)

    def test_missing_declared_material_is_rejected_before_mutation(self):
        with tempfile.TemporaryDirectory(prefix='cedar-license-declared-') as directory:
            root = Path(directory).resolve()
            _, metadata = self.material_fixture(root)
            metadata['packages'][0]['license_file'] = 'MISSING.txt'
            self.reject_unchanged(root, metadata)

    def test_stale_package_output_is_not_silently_retained_as_current(self):
        with tempfile.TemporaryDirectory(prefix='cedar-license-stale-') as directory:
            root = Path(directory).resolve()
            _, metadata = self.material_fixture(root)
            metadata['packages'] = []
            self.reject_unchanged(root, metadata)

    def test_declared_absolute_and_parent_escape_are_rejected(self):
        for kind in ('absolute', 'parent'):
            with self.subTest(kind=kind), tempfile.TemporaryDirectory(prefix='cedar-license-escape-') as directory:
                root = Path(directory).resolve()
                base, metadata = self.material_fixture(root)
                outside = base.parent / 'outside.txt'
                outside.write_bytes(b'Synthetic outside package sentinel\n')
                metadata['packages'][0]['license_file'] = str(outside) if kind == 'absolute' else '../outside.txt'
                self.reject_unchanged(root, metadata)
                self.assertEqual(outside.read_bytes(), b'Synthetic outside package sentinel\n')

    def test_unreviewed_nested_basename_cannot_overwrite_root_material(self):
        with tempfile.TemporaryDirectory(prefix='cedar-license-collision-') as directory:
            root = Path(directory).resolve()
            base, metadata = self.material_fixture(root)
            (base / 'nested').mkdir()
            (base / 'nested/LICENSE').write_bytes(b'Different nested author and license\n')
            metadata['packages'][0]['license_file'] = 'nested/LICENSE'
            self.reject_unchanged(root, metadata)

    def test_registry_identity_collision_is_rejected(self):
        with tempfile.TemporaryDirectory(prefix='cedar-license-identity-') as directory:
            root = Path(directory).resolve()
            _, metadata = self.material_fixture(root)
            second = root / 'registry/other-source'
            second.mkdir()
            (second / 'Cargo.toml').write_text('[package]\nname="example"\nversion="1.0.0"\n')
            (second / 'LICENSE').write_bytes(b'Second source must not overwrite first\n')
            metadata['packages'].append(dict(metadata['packages'][0],
                id='registry+second#example@1.0.0', source='registry+https://second.invalid/index',
                manifest_path=str(second / 'Cargo.toml')))
            self.reject_unchanged(root, metadata)

    def test_material_source_and_destination_symlinks_are_rejected(self):
        for kind in ('source', 'destination'):
            with self.subTest(kind=kind), tempfile.TemporaryDirectory(prefix='cedar-license-link-') as directory:
                root = Path(directory).resolve()
                base, metadata = self.material_fixture(root)
                outside = root / 'outside.txt'
                outside.write_bytes(b'Synthetic outside sentinel stays unchanged\n')
                path = (base / 'LICENSE' if kind == 'source' else
                        root / 'third-party-licenses/example-1.0.0/LICENSE')
                saved = path.read_bytes()
                path.unlink()
                try:
                    path.symlink_to(outside)
                except (OSError, NotImplementedError):
                    path.write_bytes(saved)
                    self.skipTest('This test host cannot create a symlink')
                self.reject_unchanged(root, metadata)
                self.assertEqual(outside.read_bytes(), b'Synthetic outside sentinel stays unchanged\n')

    def test_late_invalid_font_input_does_not_publish_partial_materials(self):
        with tempfile.TemporaryDirectory(prefix='cedar-license-late-') as directory:
            root = Path(directory).resolve()
            base, metadata = self.material_fixture(root)
            before = self.material_outputs(root)
            (base / 'LICENSE').write_bytes(b'Changed material must remain staged\n')
            (root / 'crates/app/assets/fonts/manifest.json').write_text('{invalid')
            with self.assertRaises(ValueError):
                collector.collect(root, metadata)
            self.assertEqual(self.material_outputs(root), before)


if __name__ == '__main__':
    unittest.main()
