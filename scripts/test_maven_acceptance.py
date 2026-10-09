"""Synthetic-only privacy and exact PowerShell Maven release-gate tests."""
import copy
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

import collect_java_crash as collector

PROBE_SUCCESS = {
    'model_probe_outcome': 'ready', 'event_probe_outcome': 'events_accepted',
    'model_error_code': 'none', 'model_rejection': 'none', 'event_error_code': 'none',
    'event_rejection': 'none', 'rejected_diagnostic_origin': 'none',
    'rejected_diagnostic_code_shape': 'none', 'rejected_diagnostic_message_class': 'none',
    'rejected_diagnostic_severity': 'none',
}

def good_receipt():
    def case(name):
        value = {key: True for key, kind in collector.MAVEN_CASE_FIELDS.items() if kind == 'bool'}
        value.update(PROBE_SUCCESS)
        value['owned_project_missing_library_diagnostic'] = name == 'missing'
        value.update(case=name, failure_stage='none', model_status='imported' if name == 'present' else 'unresolved',
                     model_queries=1, unexpected_dependency_references=0, generated_metadata_files=8,
                     lifecycle_metadata_files=6, lifecycle_metadata_mask=63,
                     generated_data_files=45, generated_data_bytes=45_000_000,
                     generated_project_files=0, generated_project_bytes=0, foreign_repository_files=0,
                     stop_status='graceful', stop_reason='root_exited', root_exit_code=0,
                     primary_failed=False, cleanup_failed=False)
        for key in ('dependency_jar_present_before', 'dependency_jar_present_after',
                    'dependency_pom_present_before', 'dependency_pom_present_after'):
            value[key] = name == 'present'
        if name == 'missing':
            for key in ('hover', 'completion', 'deliberate_type_diagnostic', 'dirty_change_acknowledged',
                        'no_autosave'):
                value[key] = False
        return value
    return {'kind': 'windows_java_maven', 'schema_version': 1, 'route': 'normal_agent_normal_client',
            'pair_count': 1, 'cache_input_files': 83, 'cache_input_unchanged': True,
            'fixture_inputs_verified': True, 'elapsed_ms': 20000, 'elapsed_saturated': False,
            'source_sha256': '5dda0de22c2184b1420be8e68f8a37e9165b59658d5c5cbf9fe2ee770a1003e5',
            'pom_sha256': 'c13116c2a4d7dd73f28f604480b6aad3ce818a11db526b7f61737c1c3864b65b',
            'dependency_jar_sha256': '82579c654968c77f0bd3d04c28a22b24396c35270ce76d015807410438952b5d',
            'primary_failed': False, 'cleanup_failed': False, 'success': True,
            'present': case('present'), 'missing': case('missing')}


def sanitize(value):
    return collector.sanitize_agent_transcript(json.dumps(value).encode(), collector.LIMITS)


class MavenReceiptTests(unittest.TestCase):
    @unittest.skipUnless(os.name == 'nt', 'native Windows launcher metadata')
    def test_native_launcher_environment_presence_is_names_and_booleans_only(self):
        names = ('CLIENT_PORT', 'CLIENT_HOST', 'socket.stream.debug', 'JDK_JAVA_OPTIONS',
                 'JAVA_TOOL_OPTIONS', '_JAVA_OPTIONS', 'MAVEN_OPTS', 'MAVEN_ARGS',
                 'MAVEN_CONFIG', 'MAVEN_USER_HOME', 'M2_HOME', 'MAVEN_HOME',
                 'MAVEN_PROJECTBASEDIR', 'MAVEN_CMD_LINE_ARGS', 'MAVEN_EXT_CLASS_PATH')
        present = {name: name in os.environ for name in names}
        self.assertTrue(all(type(value) is bool for value in present.values()))
        print(json.dumps({'kind': 'maven_ci_launcher_environment_presence',
                          'present': present}, sort_keys=True))

    def test_complete_pair_survives_fixed_schema_without_raw_nested_data(self):
        value = good_receipt()
        expected = copy.deepcopy(value)
        value['private_environment'] = 'SECRET_SENTINEL'
        value['present']['message'] = 'SECRET_SENTINEL'
        value['missing']['nested'] = {'source': 'SECRET_SENTINEL'}
        result, errors, truncated = sanitize(value)
        self.assertFalse(errors)
        self.assertFalse(truncated)
        self.assertEqual(result['records'], [expected])
        self.assertNotIn('SECRET_SENTINEL', json.dumps(result))

    def test_nested_type_enum_counter_and_fixed_hash_fail_closed(self):
        for case, field, invalid in [
            ('present', 'hover', 'SECRET_SENTINEL'),
            ('missing', 'owned_project_missing_library_diagnostic', 'SECRET_SENTINEL'),
            ('missing', 'owned_project_missing_library_diagnostic', 1), ('missing', 'model_queries', 241),
            ('missing', 'stop_reason', 'SECRET_SENTINEL'), ('present', 'root_exit_code', True),
            ('present', 'generated_data_bytes', 134217729),
            ('present', 'lifecycle_metadata_files', 7),
            ('missing', 'lifecycle_metadata_mask', 64),
            ('missing', 'lifecycle_metadata_mask', True),
            ('missing', 'model_error_code', 'language_maven_invalid_model: SECRET_SENTINEL'),
            ('present', 'event_rejection', 'SECRET_SENTINEL'),
            ('missing', 'generated_project_files', 257)]:
            with self.subTest(case=case, field=field):
                value = good_receipt()
                value[case][field] = invalid
                result, errors, _ = sanitize(value)
                self.assertIn('invalid_field_' + field, errors)
                self.assertNotIn(field, result['records'][0][case])
                self.assertNotIn('SECRET_SENTINEL', json.dumps(result))
        value = good_receipt()
        value['source_sha256'] = 'a' * 64
        result, errors, _ = sanitize(value)
        self.assertIn('invalid_field_source_sha256', errors)
        self.assertNotIn('source_sha256', result['records'][0])

    def test_missing_case_object_is_not_invented_and_null_failure_status_is_retained(self):
        value = good_receipt()
        value['missing'] = 'SECRET_SENTINEL'
        result, errors, _ = sanitize(value)
        self.assertIn('invalid_field_missing', errors)
        self.assertNotIn('missing', result['records'][0])
        failed = good_receipt()
        failed['success'] = False
        failed['missing'].update(success=False, stop_status=None, stop_reason=None, root_exit_code=None)
        result, errors, _ = sanitize(failed)
        self.assertFalse(errors)
        self.assertIsNone(result['records'][0]['missing']['stop_status'])
        self.assertFalse(result['records'][0]['success'])

    def test_failed_probe_retains_fixed_categories_without_raw_response(self):
        value = good_receipt()
        value.update(success=False, primary_failed=True)
        value['missing'].update(success=False, primary_failed=True,
                                event_probe_outcome='events_rejected',
                                event_rejection='unexpected_pom_diagnostic',
                                rejected_diagnostic_origin='pom',
                                rejected_diagnostic_code_shape='integer_zero',
                                rejected_diagnostic_message_class='offline_owned_dependency',
                                rejected_diagnostic_severity='error',
                                raw_model={'text': 'SECRET_SENTINEL'},
                                raw_diagnostic='SECRET_SENTINEL')
        result, errors, _ = sanitize(value)
        self.assertFalse(errors)
        rejected = result['records'][0]['missing']
        self.assertEqual(rejected['event_rejection'], 'unexpected_pom_diagnostic')
        self.assertEqual(rejected['rejected_diagnostic_code_shape'], 'integer_zero')
        self.assertNotIn('SECRET_SENTINEL', json.dumps(result))

    def test_project_marker_categories_and_boolean_are_sanitized_without_payload(self):
        value = good_receipt()
        value['missing'].update(
            event_probe_outcome='events_rejected',
            event_rejection='unexpected_project_diagnostic',
            rejected_diagnostic_origin='owned_project_root',
            rejected_diagnostic_code_shape='string_invalid_classpath',
            rejected_diagnostic_message_class='owned_missing_maven_library',
            rejected_diagnostic_severity='error',
            raw_uri='SECRET_SENTINEL', raw_message='SECRET_SENTINEL')
        result, errors, truncated = sanitize(value)
        self.assertFalse(errors)
        self.assertFalse(truncated)
        case = result['records'][0]['missing']
        self.assertIs(case['owned_project_missing_library_diagnostic'], True)
        self.assertEqual(case['rejected_diagnostic_origin'], 'owned_project_root')
        self.assertEqual(case['rejected_diagnostic_code_shape'], 'string_invalid_classpath')
        self.assertEqual(case['rejected_diagnostic_message_class'], 'owned_missing_maven_library')
        self.assertNotIn('SECRET_SENTINEL', json.dumps(result))
        for invalid in (None, 0, 1, 'true', [], {}, [True]):
            with self.subTest(invalid=invalid):
                value['missing']['owned_project_missing_library_diagnostic'] = invalid
                result, errors, _ = sanitize(value)
                self.assertIn('invalid_field_owned_project_missing_library_diagnostic', errors)
                self.assertNotIn('owned_project_missing_library_diagnostic', result['records'][0]['missing'])

    @unittest.skipUnless(shutil.which('pwsh'), 'PowerShell is required for the actual Maven release predicate')
    def test_actual_powershell_gate_rejects_missing_contradictory_or_forged_witnesses(self):
        good = good_receipt()
        cases = [{'name': 'complete', 'records': [good], 'accept': True}]
        forced = copy.deepcopy(good)
        forced['present'].update(stop_status='forced', stop_reason='grace_expired', root_exit_code=1067)
        forced['present'].update(shutdown_response_received=False, exit_frame_completed=False)
        cases.append({'name': 'honest_forced_cleanup', 'records': [forced], 'accept': True})
        subset = copy.deepcopy(good)
        subset['present'].update(lifecycle_metadata_files=2, lifecycle_metadata_mask=33)
        subset['missing'].update(lifecycle_metadata_files=0, lifecycle_metadata_mask=0)
        cases.append({'name': 'exact_marker_subsets', 'records': [subset], 'accept': True})

        def reject(name, path, value=None, remove=False):
            item = copy.deepcopy(good)
            owner = item
            for part in path[:-1]:
                owner = owner[part]
            if remove:
                del owner[path[-1]]
            else:
                owner[path[-1]] = value
            cases.append({'name': name, 'records': [item], 'accept': False})

        common = ('java_capabilities', 'generic_start_rejected', 'untrusted_start_rejected',
                  'model_without_session_rejected', 'async_start_begin_acknowledged',
                  'async_start_read_while_starting', 'async_start_ready', 'root_identity_verified',
                  'root_observed_live', 'maven_nature', 'custom_source', 'compiler_17',
                  'exact_dependency_reference', 'stale_startup_rejected', 'changed_pom_restart_required', 'source_unchanged',
                  'pom_expected', 'repository_inputs_unchanged', 'cleanup_joined',
                  'shutdown_response_received', 'exit_frame_completed', 'stop_outcome_verified',
                  'root_handle_signaled', 'model_after_stop_rejected', 'client_reaped',
                  'synthetic_root_removed', 'success')
        for name in ('present', 'missing'):
            field = 'owned_project_missing_library_diagnostic'
            reject(name + '_missing_project_witness', [name, field], remove=True)
            for invalid in (None, [], [True], 'true', 1, name == 'present'):
                reject(name + '_project_witness_' + repr(invalid), [name, field], invalid)
            for field in PROBE_SUCCESS:
                for invalid in (None, [], [PROBE_SUCCESS[field]], 'other', 'SECRET_SENTINEL'):
                    reject(name + '_probe_' + field + '_' + repr(invalid), [name, field], invalid)
                reject(name + '_probe_missing_' + field, [name, field], remove=True)
            for field in common:
                reject(name + '_' + field, [name, field], False)
            reject(name + '_missing_boolean', [name, 'source_unchanged'], remove=True)
            reject(name + '_string_boolean', [name, 'cleanup_joined'], 'true')
            for field, value in [('primary_failed', True), ('cleanup_failed', True), ('model_queries', 0),
                                 ('model_queries', 241), ('model_queries', '1'),
                                 ('unexpected_dependency_references', 1), ('foreign_repository_files', 1),
                                 ('generated_metadata_files', 129), ('generated_data_bytes', 0),
                                 ('generated_metadata_files', 5), ('lifecycle_metadata_files', 7),
                                 ('lifecycle_metadata_files', 5), ('lifecycle_metadata_files', True),
                                 ('lifecycle_metadata_mask', 64), ('lifecycle_metadata_mask', 62),
                                 ('lifecycle_metadata_mask', '63'), ('lifecycle_metadata_mask', True),
                                 ('generated_data_bytes', 134217729), ('generated_data_files', 0),
                                 ('generated_data_files', 4097), ('generated_project_bytes', 16777217),
                                 ('generated_project_files', 257), ('root_exit_code', None),
                                 ('root_exit_code', 1067), ('stop_status', None), ('stop_status', 'error'),
                                 ('stop_reason', 'grace_expired'), ('failure_stage', 'model')]:
                reject(f'{name}_{field}_{value}', [name, field], value)
            reject(name + '_missing_marker_count', [name, 'lifecycle_metadata_files'], remove=True)
            reject(name + '_missing_marker_mask', [name, 'lifecycle_metadata_mask'], remove=True)
        for field in ('hover', 'completion', 'deliberate_type_diagnostic', 'dirty_change_acknowledged',
                      'no_autosave', 'changed_pom_restart_required'):
            reject('present_' + field, ['present', field], False)
        for field in ('dependency_jar_present_before', 'dependency_jar_present_after',
                      'dependency_pom_present_before', 'dependency_pom_present_after'):
            reject('present_' + field, ['present', field], False)
            reject('missing_' + field, ['missing', field], True)
        for path, value in [(['missing', 'offline_pom_diagnostic'], False),
                            (['missing', 'model_status'], 'imported'), (['present', 'model_status'], 'unresolved'),
                            (['pair_count'], 0), (['schema_version'], True), (['cache_input_files'], 82),
                            (['cache_input_unchanged'], False), (['fixture_inputs_verified'], False),
                            (['success'], False), (['primary_failed'], True), (['cleanup_failed'], True),
                            (['elapsed_saturated'], True), (['elapsed_ms'], 360001),
                            (['route'], 'fixture_bypass'), (['source_sha256'], 'a' * 64)]:
            reject('_'.join(path), path, value)
        for field in ('kind', 'route', 'source_sha256', 'pom_sha256', 'dependency_jar_sha256'):
            for invalid in ([], [good[field]], None, 1):
                reject('top_scalar_' + field + '_' + repr(invalid), [field], invalid)
        for name in ('present', 'missing'):
            for field in ('case', 'failure_stage', 'model_status', 'stop_status', 'stop_reason'):
                for invalid in ([], [good[name][field]], None, 1):
                    reject(name + '_scalar_' + field + '_' + repr(invalid), [name, field], invalid)
            reject(name + '_object_array', [name], [good[name]])
        cases.extend([{'name': 'zero_tests', 'records': [], 'accept': False},
                      {'name': 'duplicate_pair', 'records': [good, good], 'accept': False}])
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            data = root / 'cases.json'
            data.write_text(json.dumps(cases), encoding='utf-8')
            script = root / 'predicate-test.ps1'
            script.write_text(r'''param($Predicate, $Cases)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
. $Predicate
foreach ($case in (Get-Content -LiteralPath $Cases -Raw | ConvertFrom-Json)) {
    $accepted = $false; $count = -1
    try { $count = Assert-MavenReceipt -Receipts @($case.records); $accepted = $true } catch {}
    if ($accepted -ne $case.accept -or ($accepted -and $count -ne 1)) {
        throw ('Unexpected Maven verdict: ' + $case.name)
    }
}
''', encoding='utf-8')
            result = subprocess.run([shutil.which('pwsh'), '-NoLogo', '-NoProfile', '-File', str(script),
                                     '-Predicate', str(Path(__file__).with_name('maven_acceptance_predicate.ps1').resolve()),
                                     '-Cases', str(data)], capture_output=True, text=True, timeout=60)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)


if __name__ == '__main__':
    unittest.main()
