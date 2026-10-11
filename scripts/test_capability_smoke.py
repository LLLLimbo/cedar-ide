"""Independent complete inventories; no actual agent or language process runs."""
import unittest
from pathlib import Path
import re

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
    'language_java_signature_help',
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
LINUX_GROUPS = ['java_maven_dependencies_v1', 'java_maven_leaf_v1']


class PlatformCapabilityTests(unittest.TestCase):
    def test_non_linux_shipping_inventory_omits_groups(self):
        for platform, expected in EXPECTED.items():
            if platform == 'linux':
                continue
            for groups in ([], ['java_maven_leaf_v1'], LINUX_GROUPS, ['unknown_v1'], None):
                with self.subTest(platform=platform, groups=groups):
                    with self.assertRaises(AssertionError):
                        smoke.validate_platform_capabilities({
                            'os': platform, 'capabilities': sorted(expected),
                            'capability_groups': groups})

    def test_linux_requires_exact_groups_without_flat_maven_additions(self):
        info = {'os': 'linux', 'capabilities': sorted(EXPECTED['linux'])}
        with self.assertRaises(AssertionError):
            smoke.validate_platform_capabilities(info)
        for groups in ([], None, 'java_maven_leaf_v1', ['java_maven_leaf_v1'],
                       ['java_maven_dependencies_v1'], list(reversed(LINUX_GROUPS)),
                       ['java_maven_dependencies_v1', 'java_maven_leaf_v2'],
                       ['java_maven_dependencies_v1', 'java_maven_leaf_v1.extra'],
                       LINUX_GROUPS + ['unknown_v1']):
            with self.subTest(groups=groups), self.assertRaises(AssertionError):
                smoke.validate_platform_capabilities({**info, 'capability_groups': groups})
        smoke.validate_platform_capabilities({**info, 'capability_groups': LINUX_GROUPS})

    def check(self, platform, capabilities):
        info = {'os': platform, 'capabilities': list(capabilities)}
        if platform == 'linux':
            info['capability_groups'] = LINUX_GROUPS.copy()
        smoke.validate_platform_capabilities(info)

    def test_complete_platform_sets_fit_unchanged_wire_bound(self):
        for platform, expected in EXPECTED.items():
            with self.subTest(platform=platform):
                self.check(platform, expected)
                self.assertLessEqual(len(expected), 32)
        self.assertEqual(len(EXPECTED['linux']), 32)
        self.assertEqual(len(EXPECTED['windows']), 32)

    def test_independent_inventory_covers_shipping_advertisement_sources(self):
        # Cross-check the source's bounded current builder; do not generate the
        # independent per-platform expected sets from the implementation.
        root = Path(__file__).resolve().parent.parent
        source = (root / 'crates/workspace/src/capabilities.rs').read_text(encoding='utf-8')
        builder = source.split('pub(super) fn agent_info(', 1)[1].split('capabilities.sort_unstable();', 1)[0]
        # The vec initializer has a different terminator from method calls.
        initial = re.search(r'let mut capabilities = vec!\[(.*?)\];', builder, re.DOTALL)
        self.assertIsNotNone(initial)
        advertised = set(re.findall(r'"([a-z0-9_]+)"', initial.group(1)))
        for body in re.findall(r'capabilities\.(?:push|extend)\((.*?)\);', builder, re.DOTALL):
            advertised.update(re.findall(r'"([a-z0-9_]+)"', body))
        protocol = (root / 'crates/protocol/src/lib.rs').read_text(encoding='utf-8')
        constants = set(re.findall(r'cedar_protocol::(JAVA_[A-Z_]+)', builder))
        self.assertEqual(constants, {'JAVA_STARTUP_CAPABILITIES', 'JAVA_MAVEN_CAPABILITIES',
                                     'JAVA_MAVEN_DEPENDENCIES_CAPABILITY'})
        for name in constants:
            declaration = re.search(r'pub const ' + name + r':[^=]+=(.*?);', protocol, re.DOTALL)
            self.assertIsNotNone(declaration, name)
            advertised.update(re.findall(r'"([a-z0-9_]+)"', declaration.group(1)))
        self.assertEqual(advertised, set().union(*EXPECTED.values()))
        self.assertIn('language_java_signature_help', EXPECTED['linux'])
        self.assertIn('language_java_signature_help', EXPECTED['windows'])

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

    def test_linux_typed_java_keeps_flat_inventory_with_grouped_maven(self):
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
        info['capabilities'].remove('language_java_signature_help')
        info['capabilities'].append('language_start_java')
        self.assertEqual(len(info['capabilities']), 32)
        self.assertEqual(len(set(info['capabilities'])), 31)
        with self.assertRaises(AssertionError):
            smoke.validate(hello)

    def test_group_shape_identifier_uniqueness_and_byte_bounds_stay_strict(self):
        info = {
            'schema': 1, 'version': 'inventory-test', 'os': 'linux', 'arch': 'x86_64',
            'capabilities': sorted(EXPECTED['linux']),
        }
        for groups in ([], LINUX_GROUPS, ['a' * 64, 'b' * 64], ['unknown_future_v1']):
            smoke.validate({'type': 'hello', 'protocol': 4,
                            'agent': {**info, 'capability_groups': groups}})
        for groups in (None, {}, 'java_maven_leaf_v1', [1], [True], [[]],
                       [''], ['Uppercase'], ['snow_雪'], ['newline\n'],
                       ['a' * 65], ['a', 'a'], ['a', 'b', 'c']):
            with self.subTest(groups=groups), self.assertRaises(AssertionError):
                smoke.validate({'type': 'hello', 'protocol': 4,
                                'agent': {**info, 'capability_groups': groups}})


if __name__ == '__main__':
    unittest.main()
