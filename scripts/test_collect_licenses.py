#!/usr/bin/env python3
"""Notice regeneration must retain non-Cargo fixtures and approved font licenses."""
import contextlib
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


if __name__ == '__main__':
    unittest.main()
