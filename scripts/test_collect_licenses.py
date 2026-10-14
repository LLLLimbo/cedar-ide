#!/usr/bin/env python3
"""Notice regeneration must retain non-Cargo fixtures and approved font licenses."""
import contextlib
import io
import json
from pathlib import Path
import runpy
import shutil
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parent.parent


class LicenseCollectionTests(unittest.TestCase):
    def test_regeneration_keeps_dependency_fixture_and_font_notices(self):
        with tempfile.TemporaryDirectory(prefix='cedar-notice-test-') as directory:
            root = Path(directory)
            (root / 'scripts').mkdir()
            script = root / 'scripts/collect_licenses.py'
            shutil.copyfile(ROOT / 'scripts/collect_licenses.py', script)
            fonts = root / 'crates/app/assets/fonts'
            fonts.mkdir(parents=True)
            for name in ['manifest.json', 'Inter-LICENSE.txt', 'JetBrainsMono-OFL.txt']:
                shutil.copyfile(ROOT / 'crates/app/assets/fonts' / name, fonts / name)
            dependency = root / 'dependency'
            dependency.mkdir()
            (dependency / 'LICENSE').write_text('Synthetic dependency license\n')
            metadata = {'packages': [
                {'name': 'cedar-example', 'version': '0.71.0', 'source': None},
                {'name': 'synthetic-dependency', 'version': '1.0', 'source': 'registry',
                 'manifest_path': str(dependency / 'Cargo.toml'), 'license': 'MIT',
                 'repository': 'https://example.invalid/dependency'},
            ]}
            previous = None
            for _ in range(2):
                stdout = io.StringIO()
                with mock.patch('subprocess.check_output', return_value=json.dumps(metadata).encode()) as cargo:
                    with contextlib.redirect_stdout(stdout):
                        runpy.run_path(str(script), run_name='__main__')
                cargo.assert_called_once_with(
                    ['cargo', 'metadata', '--format-version', '1', '--locked', '--offline'], cwd=root)
                self.assertIn('Collected notices for 1 locked packages', stdout.getvalue())
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
                previous = text
                self.assertEqual((root / 'third-party-licenses/synthetic-dependency-1.0/LICENSE').read_text(),
                                 'Synthetic dependency license\n')


if __name__ == '__main__':
    unittest.main()
