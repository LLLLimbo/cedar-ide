"""Pure expectations; these do not claim actual Windows process execution."""
import unittest

import capability_smoke as smoke


class PlatformCapabilityTests(unittest.TestCase):
    def windows(self):
        return smoke.BASE | smoke.TASKS | smoke.WINDOWS_JAVA | smoke.WINDOWS_MAVEN | {
            'git_changes', 'git_diff'}

    def check(self, platform, capabilities):
        smoke.validate_platform_capabilities({'os': platform, 'capabilities': list(capabilities)})

    def test_normal_windows_typed_routes_are_expected(self):
        self.check('windows', self.windows())

    def test_generic_windows_execution_remains_rejected(self):
        for capability in ('language_start', 'run', 'git_status'):
            with self.subTest(capability=capability), self.assertRaises(AssertionError):
                self.check('windows', self.windows() | {capability})

    def test_missing_typed_routes_or_task_capabilities_fail(self):
        for capability in smoke.WINDOWS_JAVA | smoke.WINDOWS_MAVEN | smoke.TASKS:
            with self.subTest(capability=capability), self.assertRaises(AssertionError):
                self.check('windows', self.windows() - {capability})

    def test_old_all_language_disabled_expectation_is_not_accepted(self):
        with self.assertRaises(AssertionError):
            self.check('windows', smoke.BASE | smoke.TASKS | {'git_changes', 'git_diff'})

    def test_unix_existing_requirements_remain(self):
        capabilities = smoke.BASE | smoke.TASKS | smoke.LANGUAGE | {
            'run', 'git_status', 'git_changes', 'git_diff'}
        for platform in ('linux', 'macos'):
            self.check(platform, capabilities)
            with self.assertRaises(AssertionError):
                self.check(platform, capabilities - {'language_start'})


if __name__ == '__main__':
    unittest.main()
