"""Independent complete inventories; no actual agent or language process runs."""
import unittest

import capability_smoke as smoke


# Spell out the complete baseline and the additions independently of smoke's
# implementation constants. Missing query capabilities must fail too.
UNIX = {
    'list', 'read', 'write', 'search', 'git_status', 'git_changes', 'git_diff',
    'run', 'run_start', 'run_poll', 'run_cancel', 'language_start',
    'language_open', 'language_change', 'language_close', 'language_events',
    'language_stop', 'language_query', 'language_format', 'language_references',
    'language_document_symbols', 'language_workspace_symbols',
    'language_resolve_uri', 'language_resolve_completion',
}
JAVA = {
    'language_start_java', 'language_start_java_begin', 'language_start_java_poll',
    'language_start_java_cancel', 'java_diagnostics_refresh',
    'language_organize_java_imports', 'language_java_implementations',
}
MAVEN = {
    'language_start_java_maven_begin', 'language_maven_model',
    'language_maven_dependencies',
}
EXPECTED = {
    'linux': UNIX | JAVA,
    'macos': UNIX,
    'windows': (UNIX - {'run', 'git_status', 'language_start'}) | JAVA | MAVEN,
    'other': UNIX - {'run', 'git_status', 'git_changes', 'git_diff',
                     'run_start', 'run_poll', 'run_cancel'},
}


class PlatformCapabilityTests(unittest.TestCase):
    def check(self, platform, capabilities):
        smoke.validate_platform_capabilities({'os': platform, 'capabilities': list(capabilities)})

    def test_complete_platform_sets_fit_unchanged_wire_bound(self):
        for platform, expected in EXPECTED.items():
            with self.subTest(platform=platform):
                self.check(platform, expected)
                self.assertLessEqual(len(expected), 32)

    def test_every_advertised_capability_is_required(self):
        for platform, expected in EXPECTED.items():
            for capability in expected:
                with self.subTest(platform=platform, capability=capability):
                    with self.assertRaises(AssertionError):
                        self.check(platform, expected - {capability})

    def test_every_foreign_capability_is_rejected(self):
        inventory = set().union(*EXPECTED.values(), {'terminal'})
        for platform, expected in EXPECTED.items():
            for capability in inventory - expected:
                with self.subTest(platform=platform, capability=capability):
                    with self.assertRaises(AssertionError):
                        self.check(platform, expected | {capability})

    def test_linux_typed_java_does_not_inherit_maven(self):
        self.check('linux', UNIX | JAVA)
        with self.assertRaises(AssertionError):
            self.check('linux', UNIX)
        with self.assertRaises(AssertionError):
            self.check('linux', UNIX | JAVA | MAVEN)

    def test_metadata_wire_capacity_and_duplicate_names(self):
        info = {
            'schema': 1, 'version': 'inventory-test', 'os': 'linux', 'arch': 'x86_64',
            'capabilities': sorted(EXPECTED['linux']),
        }
        hello = {'type': 'hello', 'protocol': 4, 'agent': info}
        smoke.validate(hello)
        while len(info['capabilities']) < 32:
            info['capabilities'].append(f'fixture_capacity_{len(info["capabilities"])}')
        smoke.validate(hello)
        info['capabilities'].append('fixture_over_capacity')
        with self.assertRaises(AssertionError):
            smoke.validate(hello)
        info['capabilities'] = sorted(EXPECTED['linux'])
        info['capabilities'].append('language_start_java')
        with self.assertRaises(AssertionError):
            smoke.validate(hello)


if __name__ == '__main__':
    unittest.main()
