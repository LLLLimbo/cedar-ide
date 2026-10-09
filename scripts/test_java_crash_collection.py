#!/usr/bin/env python3
"""Synthetic-only checks for the sanitized JVM crash evidence collector."""
import hashlib
from contextlib import contextmanager
import json
import os
from pathlib import Path, PureWindowsPath
import shutil
import stat
import subprocess
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock

import collect_java_crash as collector


HOTSPOT = '''#
# A fatal error has been detected by the Java Runtime Environment:
#
#  EXCEPTION_ACCESS_VIOLATION (0xc0000005) at pc=0x00007fff01234567, pid=314, tid=159
#
# JRE version: OpenJDK Runtime Environment Temurin-21.0.8+9 (21.0.8+9) (build 21.0.8+9-LTS)
# Java VM: OpenJDK 64-Bit Server VM Temurin-21.0.8+9 (21.0.8+9-LTS, mixed mode, windows-amd64)
# Problematic frame:
# C  [C:\\synthetic-private-path\\jdk\\bin\\server\\jvm.dll+0x123abc] JVM_Synthetic+0x5
#
# No core dump will be written. Minidumps are not enabled by default.
#
---------------  S U M M A R Y ------------
Command Line: -Dtoken=SECRET_COMMANDLINE org.example.Main
Host: SECRET_HOST, 4 cores
---------------  T H R E A D  ---------------
Current thread: SECRET_THREAD
Stack: [0x00000000,0x11111111], sp=0x22222222
Native frames: (J=compiled Java code, j=interpreted, Vv=VM code, C=native code)
C  [jvm.dll+0x123abc] JVM_Synthetic+0x5
V  [jvm.dll+0x456def]
C  0x00007fff11223344
j  org.example.GeneratedFixture.run()V+12
v  ~StubRoutines::call_stub

Java frames: (J=compiled Java code, j=interpreted, Vv=VM code)
J 147 c2 org.example.GeneratedFixture.run()V (42 bytes) @ 0x000077777777 [0x000088888888+0x0000000001]
j  java.lang.Thread.run()V+0

siginfo: SECRET_SIGINFO
Registers:
RAX=0xSECRET_REGISTER, RBX=0x22222222
Top of Stack: (sp=0x22222222)
0x22222222: SECRET_STACK_MEMORY
Instructions: SECRET_INSTRUCTION_DUMP
Environment Variables:
SECRET_ENVIRONMENT_TOKEN=synth-secret-123
System Properties:
user.home=SECRET_HOME
java.class.path=SECRET_CLASSPATH
Dynamic libraries:
C:\\SECRET_USER\\SECRET_DLL.dll
VM Arguments:
jvm_args: -Dsecret=SECRET_ARGUMENTS
'''


class CrashCollectionTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='cedar-crash-test-')
        self.addCleanup(self.temporary.cleanup)
        # All contents, including deliberately out-of-scan-root link targets,
        # are synthetic files under this one owned temporary test directory.
        self.base = Path(self.temporary.name)
        self.root = self.base / 'scratch'
        self.root.mkdir()

    def log(self, content=HOTSPOT, name='hs_err_pid314.log'):
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(content if isinstance(content, bytes) else content.encode('utf-8'))
        return path

    def limits(self, **changes):
        return dict(collector.LIMITS, **changes)

    def private_source(self, name, contents):
        path = self.root / name
        path.write_bytes(contents if isinstance(contents, bytes) else contents.encode('utf-8'))
        return path

    def probe_fixture(self):
        prefix = ('SECRET_PRE_HEADER\n' + HOTSPOT).encode('utf-8')
        capture = {'observed_bytes': len(prefix), 'retained_bytes': len(prefix),
                   'retention_limit_bytes': 65536, 'truncated': False, 'eof_observed': True,
                   'prefix_hex': prefix.hex(), 'prefix_utf8_lossy': prefix.decode()}
        return {
            'driver_status': 'diagnostic_only', 'driver_completed': True,
            'acceptance_claimed': False, 'case_count': 1,
            'scratch_directory': 'SECRET_SCRATCH_PATH', 'canonical_executable': 'SECRET_JAVA_PATH',
            'cases': [{
                'name': 'hello_ordinary_unicode_cwd', 'outcome': 'child_nonzero_exit',
                'exit_code': 3221225477, 'exit_code_hex': 'SECRET_RAW_HEX',
                'job_active_processes_final': 0, 'job_zero_observed': True,
                'cleanup_verified': True, 'root_joined': True,
                'capture_cancellation_completed': True, 'stdin_cancellation_completed': True,
                'stdin_expected_bytes': 13, 'stdin_accepted_bytes': 13,
                'stdin_eof_sent_before_cleanup': True,
                'hello_markers': {'stdout_ready': True, 'stderr_ready': True, 'stdin_echo': False,
                                  'stdout_done': False, 'stderr_done': False, 'unknown': 'SECRET_MARKER'},
                'stdout': capture,
                'stderr': {'observed_bytes': 10, 'retained_bytes': 10, 'truncated': False,
                           'prefix_hex': b'SECRET_ERR'.hex(), 'prefix_utf8_lossy': 'SECRET_ERR'},
                'arguments': ['-Dtoken=SECRET_ARGUMENT'], 'errors': ['SECRET_DRIVER_ERROR'],
                'fatal_error_headers': [{'header_hex': b'SECRET_HEADER'.hex(),
                                         'header_utf8_lossy': 'SECRET_HEADER'}],
            }],
        }

    def agent_fixture(self):
        sessions = [{
            'kind': 'windows_java_session', 'session': number, 'mode': mode,
            'initialization_ms': 1000 * number,
            'semantic_checks_passed': True, 'exact_diagnostics': True, 'exact_definition': True,
            'real_completion': True, 'deferred_import_resolve': True,
            'primary_identity_unchanged': True, 'two_atomic_edits': True,
            'advisory_command_skipped': True, 'actual_undo': True, 'actual_redo': True,
            'versions_2_3_4_synced': True, 'correction_change_acknowledged': True,
            'correction_change_result': 'acknowledged', 'correction_diagnostics': True, 'source_unchanged': True,
            'workflow_success': True, 'correction_recovery_result': 'not_attempted',
            'correction_recovery_attempts': 0, 'correction_recovery_acknowledged': False,
            'correction_recovery_witness': False, 'correction_recovery_unversioned': False,
            'correction_recovery_budget_sufficient': False,
            'root_observed_live': True, 'root_identity_verified': True, 'jdk_symbol_verified': True,
            'shutdown_api_succeeded': True, 'root_handle_signaled': True, 'gracefully_exited': True,
            'root_exit_code': 0, 'shutdown_elapsed_ms': 10 * number,
        } for number, mode in enumerate(('initial', 'fresh_data', 'reused_data'), 1)]
        return sessions + [{
            'kind': 'windows_java_cleanup', 'sessions_completed': 3, 'agent_exit_zero': True,
            'source_unchanged': True, 'observed_roots_exited': True, 'synthetic_root_removed': True,
            'success': True, 'primary_failed': False, 'cleanup_failed': False, 'failure_stage': 'none',
            'spontaneous_success': True, 'workflow_success': True,
        }]

    def agent_source(self, records):
        return self.private_source('private-agent.txt', '\n'.join(json.dumps(record) for record in records))

    def agent_diagnostics_fixture(self):
        return {
            'kind': 'windows_java_diagnostics', 'session': 1, 'phase': 'initial', 'result': 'matched',
            'elapsed_ms': 1000, 'elapsed_saturated': False,
            'polls': 1, 'events': 2, 'diagnostic_batches': 3, 'uri_match_batches': 4,
            'parsed_batches': 5, 'version_match_batches': 6, 'unversioned_batches': 7,
            'eligible_batches': 8, 'eligible_empty_batches': 9, 'eligible_error_free_batches': 10,
            'error_diagnostics': 11, 'warning_diagnostics': 12, 'expected_message_diagnostics': 13,
            'expected_severity_diagnostics': 14, 'expected_range_diagnostics': 15,
            'expected_joint_diagnostics': 16, 'eligible_expected_joint_diagnostics': 17,
            'eligible_error_diagnostics': 18, 'matching_batches': 19, 'counters_saturated': False,
        }

    def correction_recovery_fixture(self):
        return {
            'kind': 'windows_java_correction_recovery', 'session': 2,
            'original_result': 'timeout', 'result': 'matched', 'attempts': 1,
            'acknowledged': True, 'witness': True, 'unversioned': True,
            'budget_sufficient': True, 'available_budget_ms': 200000,
            'required_budget_ms': 165000, 'request_timeout_ms': 75000,
            'witness_dispatch_window_ms': 15000, 'event_poll_timeout_ms': 75000,
            'cleanup_reserve_guaranteed': False, 'elapsed_ms': 1100,
            'elapsed_saturated': False, 'polls': 2, 'events': 1,
            'counters_saturated': False,
        }

    def agent_lifecycle_fixtures(self):
        return [{
            'kind': 'windows_java_concurrency', 'tasks_started': 2, 'tasks_completed': 2,
            'language_stop_preserved_task': True, 'task_cancel_preserved_java': True,
            'hover_after_cancel': True, 'task_identities_verified': True, 'task_locks_verified': True,
            'tasks_exited': True, 'task_locks_released': True, 'task_caps_not_reached': True,
            'source_unchanged': True, 'primary_failed': False, 'cleanup_failed': False,
            'success': True, 'failure_stage': 'none',
        }, {
            'kind': 'windows_java_forced_cleanup', 'java_observed_live': True,
            'java_identity_verified': True, 'task_observed_live': True, 'task_identity_verified': True,
            'task_lock_verified': True, 'owner_death_injected': True, 'agent_exit_observed': True,
            'agent_exit_nonzero': True, 'java_exit_observed': True, 'task_exit_observed': True,
            'task_lock_released': True, 'task_cap_not_reached': True, 'source_unchanged': True,
            'synthetic_root_removed': True, 'primary_failed': False, 'cleanup_failed': False,
            'success': True, 'java_exit_code': 3221225786, 'task_exit_code': 1,
            'failure_stage': 'none', 'elapsed_ms': 1250, 'elapsed_saturated': False,
        }]

    def agent_production_fixture(self):
        return {
            'kind': 'windows_java_production', 'route': 'normal_agent_client',
            'async_start_exercised': True, 'async_start_begin_acknowledged': True,
            'async_start_read_while_starting': True, 'async_start_ready': True,
            'java_capabilities': True, 'generic_start_rejected': True, 'untrusted_start_rejected': True,
            'root_observed_live': True, 'root_identity_verified': True, 'semantic_diagnostics': True,
            'exact_definition': True, 'real_completion': True, 'deferred_import_resolve': True,
            'actual_editor_apply_undo_redo': True, 'versions_2_3_4_synced': True,
            'correction_acknowledged': True, 'correction_diagnostics': True, 'source_unchanged': True,
            'diagnostics_refresh_exercised': True, 'diagnostics_refresh_supported': True,
            'diagnostics_refresh_requested': True, 'diagnostics_refresh_witness': True,
            'diagnostics_refresh_unversioned': True,
            'stop_outcome_verified': True, 'shutdown_response_received': True, 'exit_frame_completed': True,
            'cleanup_joined': True, 'root_handle_signaled': True, 'client_reaped': True,
            'synthetic_root_removed': True, 'primary_failed': False, 'cleanup_failed': False,
            'success': True, 'stop_status': 'graceful', 'stop_reason': 'root_exited', 'root_exit_code': 0,
            'failure_stage': 'none', 'elapsed_ms': 1500, 'elapsed_saturated': False,
        }

    def agent_idle_fixture(self, recovered=False):
        record = {**self.agent_production_fixture(),
            'kind': 'windows_java_idle_correction',
            'async_start_exercised': False, 'async_start_begin_acknowledged': False,
            'async_start_read_while_starting': False, 'async_start_ready': False,
            'diagnostics_refresh_exercised': False, 'diagnostics_refresh_requested': False,
            'diagnostics_refresh_witness': False, 'diagnostics_refresh_unversioned': False,
            'spontaneous_result': 'matched', 'spontaneous_success': True,
            'spontaneous_matching_batches': 1, 'recovery_attempts': 0,
            'recovery_result': 'not_attempted', 'recovery_acknowledged': False,
            'recovery_witness': False, 'recovery_unversioned': False,
            'recovery_budget_sufficient': False, 'recovery_available_budget_ms': 0,
            'workflow_success': True, 'primary_deadline_ms': 360000,
            'outer_deadline_ms': 480000, 'cleanup_reserve_ms': 120000,
            'request_timeout_ms': 75000, 'spontaneous_dispatch_window_ms': 60000,
            'diagnostic_wait_admission_ms': 135000, 'initial_idle_ms': 30000,
            'recovery_budget_ms': 165000, 'recovery_admission_ms': 240000,
            'close_budget_ms': 75000, 'stop_budget_ms': 75000,
            'root_exit_budget_ms': 3000, 'client_reap_budget_ms': 30000,
            'cleanup_bookkeeping_ms': 9000, 'primary_deadline_met': True,
            'cleanup_deadline_met': True, 'cleanup_reserve_preserved': True,
            'deadline_failed': False, 'primary_elapsed_ms': 40000,
            'cleanup_started_ms': 40000, 'elapsed_ms': 41000,
        }
        if recovered:
            record.update(correction_diagnostics=False, spontaneous_result='timeout',
                spontaneous_success=False, spontaneous_matching_batches=0,
                recovery_attempts=1, recovery_result='matched', recovery_acknowledged=True,
                recovery_witness=True, recovery_unversioned=True, recovery_budget_sufficient=True,
                recovery_available_budget_ms=250000, primary_elapsed_ms=120000,
                cleanup_started_ms=120000, elapsed_ms=121000)
        return record

    def agent_gc_control_fixture(self):
        return {**self.agent_production_fixture(), 'kind': 'windows_java_gc_control',
                'route': 'diagnostic_agent_normal_client'}

    def agent_workspace_type_fixture(self):
        return {
            'kind': 'windows_java_workspace_types', 'exercised': True,
            'capability_supported': True, 'provider_supported': True, 'target_unopened': True,
            'exact_type_name': True, 'exact_type_uri': True, 'exact_declaration_range': True,
            'negative_query_empty': True, 'resolved_path_exact': True, 'ordinary_read_exact': True,
            'actual_frontend_navigation': True, 'dirty_buffer_reused': True,
            'undo_redo_preserved': True, 'source_unchanged': True,
            'root_handle_signaled': True, 'client_reaped': True, 'synthetic_root_removed': True,
            'primary_failed': False, 'cleanup_failed': False, 'success': True,
            'failure_stage': 'none', 'elapsed_ms': 1234, 'elapsed_saturated': False,
        }

    def agent_organize_fixture(self):
        return {
            'kind': 'windows_java_organize_imports', 'exercised': True, 'supported': True,
            'left_candidate_indexed': True, 'right_candidate_indexed': True,
            'unsaved_type_indexed': True, 'independent_type_indexed': True,
            'unsaved_version_acknowledged': True, 'sorted_retained_imports': True,
            'unused_import_removed': True, 'unsaved_unique_import_added': True,
            'preview_unchanged': True, 'cancel_unchanged': True, 'actual_frontend_apply': True,
            'one_undo_exact': True, 'one_redo_exact': True, 'draft_versions_synced': True,
            'ambiguous_candidates_skipped': True, 'independent_import_added': True,
            'source_files_unchanged': True, 'root_handle_signaled': True, 'client_reaped': True,
            'synthetic_root_removed': True, 'primary_failed': False, 'cleanup_failed': False,
            'success': True, 'elapsed_saturated': False, 'main_edit_count': 3,
            'ambiguity_edit_count': 1, 'observed_editor_stages': 5,
            'failure_stage': 'none', 'elapsed_ms': 1234,
        }

    def link(self, target, path, directory=False):
        try:
            path.symlink_to(target, target_is_directory=directory)
        except (OSError, NotImplementedError) as error:
            self.skipTest('This runner does not permit symlink creation: ' + type(error).__name__)

    def test_windows_crlf_has_hash_headers_and_sanitized_frames(self):
        data = HOTSPOT.replace('\n', '\r\n').encode('utf-8')
        self.log(data, 'nested/hs_err_pid314.log')
        report = collector.collect(self.root)
        self.assertEqual(report['status'], 'complete')
        self.assertEqual(report['acceptance_result'], 'not_evaluated')
        self.assertEqual(len(report['files']), 1)
        item = report['files'][0]
        self.assertEqual(item['status'], 'collected')
        self.assertEqual(item['relative_filename'], 'nested/hs_err_pid314.log')
        self.assertEqual(item['bytes'], len(data))
        self.assertEqual(item['sha256'], hashlib.sha256(data).hexdigest())
        evidence = item['evidence']
        self.assertTrue(evidence['fatal_error'])
        self.assertEqual(evidence['exception']['name'], 'EXCEPTION_ACCESS_VIOLATION')
        self.assertEqual(evidence['exception']['code'], '0xc0000005')
        self.assertEqual(evidence['headers']['jre']['version'], '21.0.8+9-LTS')
        self.assertEqual(evidence['headers']['vm']['name'], 'OpenJDK 64-Bit Server VM')
        self.assertEqual(evidence['problematic_frame'][0]['library'], 'jvm.dll')
        self.assertEqual(evidence['problematic_frame'][0]['offset'], '0x123abc')
        self.assertEqual(len(evidence['native_frames']), 5)
        self.assertEqual(len(evidence['java_frames']), 2)
        self.assertEqual(evidence['java_frames'][0]['method'], 'org.example.GeneratedFixture.run()V')
        rendered = json.dumps(report)
        for omitted in ('SECRET_', 'synthetic-private-path', 'Registers:', 'Environment Variables:',
                        'System Properties:', 'Command Line:', 'Top of Stack:', str(self.root)):
            self.assertNotIn(omitted, rendered)

    def test_sensitive_text_inside_frame_sections_does_not_leak(self):
        content = HOTSPOT.replace('V  [jvm.dll+0x456def]', '\n'.join((
            'C  [jvm.dll+0x987] SECRET_FRAME_TOKEN=123',
            'j  org.example.GeneratedFixture.run()V SECRET_JAVA_TRAILER=123',
            'V  Registers: SECRET_FAKE_REGISTER',
            'V  [jvm.dll+0x456def]',
        )))
        self.log(content)
        report = collector.collect(self.root)
        self.assertEqual(report['status'], 'complete')
        self.assertNotIn('SECRET_', json.dumps(report))
        self.assertEqual(report['files'][0]['evidence']['omitted_unrecognized_frames'], 1)

    def test_cpp_frame_keeps_module_offset_without_arbitrary_trailing_text(self):
        frame = collector.parse_frame('V  [libjvm.so+0xabc] AccessInternal::resolve<42>(void*)+0xff SECRET_TRAILER')
        self.assertEqual(frame, {'kind': 'V', 'library': 'libjvm.so', 'offset': '0xabc'})
        frame = collector.parse_frame('j  org.example.Fixture.run([Ljava/lang/String;I)Ljava/lang/Object;+42')
        self.assertEqual(frame['method'], 'org.example.Fixture.run([Ljava/lang/String;I)Ljava/lang/Object;')
        self.assertIsNone(collector.parse_frame('j  org.example.Fixture.run()VSECRET_INVALID_DESCRIPTOR'))

    def test_register_or_memory_section_ends_frame_capture(self):
        self.log(HOTSPOT.replace('V  [jvm.dll+0x456def]',
                                'Registers:\nC  [SHOULD_NOT_APPEAR.dll+0x789]'))
        report = collector.collect(self.root)
        self.assertNotIn('SHOULD_NOT_APPEAR', json.dumps(report))

    def test_absent_logs_are_valid_empty_diagnostics(self):
        (self.root / 'unrelated.txt').write_text('SECRET_UNRELATED_FILE')
        (self.root / 'hs_err_pid314.dmp').write_bytes(b'SECRET_MINIDUMP')
        report = collector.collect(self.root)
        self.assertEqual(report['status'], 'complete')
        self.assertEqual(report['files'], [])
        self.assertEqual(report['issues'], [])
        self.assertNotIn('SECRET_', json.dumps(report))

    def test_malformed_and_non_utf8_logs_are_errors_with_hashes(self):
        self.log('SECRET_NOT_A_FATAL_LOG', 'hs_err_pid1.log')
        self.log(b'\xff\xfeSECRET_BAD_UTF8', 'hs_err_pid2.log')
        self.log(b'\x00SECRET_BINARY', 'hs_err_pid3.log')
        report = collector.collect(self.root)
        self.assertEqual(report['status'], 'error')
        self.assertEqual({item['status'] for item in report['files']}, {'error'})
        self.assertTrue(all(item['sha256'] for item in report['files']))
        self.assertNotIn('SECRET_', json.dumps(report))

    def test_unrelated_windows_acp_bytes_do_not_discard_fatal_fields(self):
        self.log(HOTSPOT.encode() + b'\nEnvironment Variables:\nSECRET_ACP=\x81\xff\x96\n')
        report = collector.collect(self.root)
        self.assertEqual(report['status'], 'complete')
        self.assertEqual(report['files'][0]['evidence']['exception']['code'], '0xc0000005')
        self.assertNotIn('SECRET_', json.dumps(report))

    def test_probe_report_preserves_only_typed_metadata_and_sanitized_fatal_prefix(self):
        raw = json.dumps(self.probe_fixture()).encode()
        path = self.private_source('private-probe.json', raw)
        report = collector.collect(self.root, probe_report=path)
        self.assertEqual(report['status'], 'complete')
        source = report['probe_report']
        self.assertEqual(source['status'], 'collected')
        self.assertFalse(source['truncated'])
        self.assertEqual(source['bytes'], len(raw))
        self.assertEqual(source['sha256'], hashlib.sha256(raw).hexdigest())
        case = source['evidence']['cases'][0]
        self.assertEqual(case['exit_code'], 3221225477)
        self.assertEqual(case['exit_code_hex'], '0xC0000005')
        self.assertEqual(case['outcome'], 'child_nonzero_exit')
        self.assertTrue(case['cleanup_verified'])
        self.assertTrue(case['job_zero_observed'])
        self.assertFalse(case['hello_markers']['stdin_echo'])
        self.assertEqual(case['stdout']['fatal_evidence']['exception']['code'], '0xc0000005')
        self.assertEqual(case['stdout']['fatal_evidence']['problematic_frame'][0]['library'], 'jvm.dll')
        self.assertFalse(case['stderr']['fatal_header_observed'])
        rendered = json.dumps(report)
        for secret in ('SECRET_', 'prefix_hex', 'prefix_utf8_lossy', 'header_hex',
                       'header_utf8_lossy', 'arguments', 'synthetic-private-path', b'SECRET_HEADER'.hex()):
            self.assertNotIn(secret, rendered)
        self.assertEqual(report['acceptance_result'], 'not_evaluated')

    def test_probe_utf8_only_prefix_fallback_and_capture_truncation(self):
        probe = self.probe_fixture()
        del probe['cases'][0]['stdout']['prefix_hex']
        probe['cases'][0]['stdout']['truncated'] = True
        path = self.private_source('private-probe.json', json.dumps(probe))
        report = collector.collect(self.root, probe_report=path)
        self.assertEqual(report['status'], 'partial')
        source = report['probe_report']
        self.assertEqual(source['status'], 'truncated')
        self.assertTrue(source['truncated'])
        self.assertEqual(source['evidence']['cases'][0]['stdout']['fatal_evidence']['exception']['name'],
                         'EXCEPTION_ACCESS_VIOLATION')
        self.assertNotIn('SECRET_', json.dumps(report))

    def test_stderr_fatal_fallback_does_not_require_error_file(self):
        probe = self.probe_fixture()
        case = probe['cases'][0]
        case['stdout'], case['stderr'] = case['stderr'], case['stdout']
        path = self.private_source('private-probe.json', json.dumps(probe))
        report = collector.collect(self.root, probe_report=path)
        self.assertEqual(report['files'], [])
        case = report['probe_report']['evidence']['cases'][0]
        self.assertTrue(case['stderr']['fatal_header_observed'])
        self.assertEqual(case['stderr']['fatal_evidence']['exception']['code'], '0xc0000005')
        self.assertNotIn('SECRET_', json.dumps(report))

    def test_probe_unknown_labels_malformed_fields_and_raw_errors_do_not_leak(self):
        probe = self.probe_fixture()
        probe['cases'][0]['hello_markers']['stdin_echo'] = 'SECRET_INVALID_BOOLEAN'
        probe['cases'][0]['cleanup_verified'] = 1
        probe['cases'][0]['exit_code'] = 'SECRET_INVALID_EXIT'
        probe['cases'][0]['stdout']['prefix_hex'] = 'SECRET_INVALID_HEX'
        probe['cases'].append({'name': 'SECRET_CASE', 'outcome': 'SECRET_OUTCOME'})
        probe['case_count'] = 2
        path = self.private_source('private-probe.json', json.dumps(probe))
        report = collector.collect(self.root, probe_report=path)
        self.assertEqual(report['status'], 'error')
        source = report['probe_report']
        self.assertIn('unknown_probe_case', source['errors'])
        self.assertIn('invalid_field_cleanup_verified', source['errors'])
        self.assertIn('invalid_prefix_hex', source['errors'])
        case = source['evidence']['cases'][0]
        self.assertNotIn('cleanup_verified', case)
        self.assertNotIn('exit_code', case)
        self.assertNotIn('SECRET_', json.dumps(report))

    def test_java_transcript_retains_seven_whitelisted_kinds_and_honest_booleans(self):
        values = [
            {'kind': 'pass', 'sessions': 3, 'windows_full_acceptance': False,
             'source_bytes_unchanged': True, 'fixture_removed': True, 'payload': 'SECRET_PASS_PAYLOAD'},
            {'kind': 'session_semantics_pass', 'session': 'initial', 'initial_diagnostics': 2,
             'corrected_diagnostics': 1, 'jdk_index_symbol_checked': True, 'checks': ['initialize', 'didOpen', 'jdk_workspace_symbol'], 'error': 'SECRET_SEMANTIC_ERROR'},
            {'kind': 'session_cleanup', 'session': 'initial', 'semantics_succeeded': False,
             'gracefully_exited': False, 'root_exit_code': 3221225477, 'source_unchanged': True,
             'shutdown_elapsed_ms': 10002, 'shutdown_terminal_reason': 'grace_expired',
             'independent_job_zero_observation': False, 'listener_observation': False},
            {'kind': 'source_bytes', 'phase': 'after_all_clients_dropped', 'unchanged': True},
            {'kind': 'fixture_cleanup', 'removed': True, 'sessions_succeeded': False,
             'source_unchanged_before_removal': True},
            {'kind': 'process_identity', 'session': 'restart_fresh_data', 'pid': 123,
             'creation_time_100ns_since_1601': 123456789123456789,
             'retained_windows_observation_handle': True, 'command_line': 'SECRET_PROCESS_COMMAND'},
            {'kind': 'data_directory_witness', 'session': 'restart_same_data',
             'metadata_in_expected_directory': True, 'data_directory': 'SECRET_DATA_PATH'},
            {'kind': 'diagnostics', 'payload': 'SECRET_DIAGNOSTICS'},
            {'kind': 'notification', 'payload': 'SECRET_NOTIFICATION'},
            {'kind': 'completion', 'payload': 'SECRET_COMPLETION'},
            {'kind': 'run_metadata', 'literal_launch_arguments_before_data': ['SECRET_ARGUMENTS']},
        ]
        raw = '\r\n'.join(['SECRET_NON_JSON'] + [json.dumps(value) for value in values]).encode()
        path = self.private_source('private-java.txt', raw)
        report = collector.collect(self.root, java_transcript=path)
        self.assertEqual(report['status'], 'complete')
        source = report['java_transcript']
        self.assertEqual(source['sha256'], hashlib.sha256(raw).hexdigest())
        records = source['evidence']['records']
        self.assertEqual(len(records), 7)
        self.assertFalse(records[0]['windows_full_acceptance'])
        self.assertTrue(records[1]['jdk_index_symbol_checked'])
        self.assertIn('jdk_workspace_symbol', records[1]['checks'])
        self.assertFalse(records[2]['gracefully_exited'])
        self.assertEqual(records[2]['root_exit_code'], 3221225477)
        self.assertEqual(records[2]['shutdown_elapsed_ms'], 10002)
        self.assertEqual(records[2]['shutdown_terminal_reason'], 'grace_expired')
        self.assertFalse(records[4]['sessions_succeeded'])
        self.assertTrue(records[6]['metadata_in_expected_directory'])
        self.assertEqual(source['evidence']['omitted_non_json_lines'], 1)
        self.assertEqual(source['evidence']['omitted_other_json_records'], 4)
        self.assertNotIn('SECRET_', json.dumps(report))
        self.assertEqual(report['acceptance_result'], 'not_evaluated')

    def test_java_transcript_invalid_scalars_enums_and_json_are_errors(self):
        raw = '\n'.join((
            json.dumps({'kind': 'session_cleanup', 'session': 'SECRET_SESSION',
                        'source_unchanged': 'SECRET_BOOLEAN', 'root_exit_code': -1,
                        'shutdown_terminal_reason': 'SECRET_TERMINAL_REASON', 'shutdown_elapsed_ms': -1}),
            json.dumps({'kind': 'pass', 'elapsed_ms': 2 ** 80}),
            '{"kind":"pass","SECRET_BROKEN_JSON":',
        ))
        path = self.private_source('private-java.txt', raw)
        report = collector.collect(self.root, java_transcript=path)
        self.assertEqual(report['status'], 'error')
        self.assertIn('malformed_json_line', report['java_transcript']['errors'])
        self.assertIn('invalid_field_shutdown_terminal_reason', report['java_transcript']['errors'])
        self.assertIn('invalid_field_shutdown_elapsed_ms', report['java_transcript']['errors'])
        self.assertNotIn('SECRET_', json.dumps(report))

    def test_optional_source_limits_remain_visible(self):
        path = self.private_source('private-java.txt', '\n'.join(
            [json.dumps({'kind': 'fixture_cleanup', 'removed': False})] * 3))
        report = collector.collect(self.root, self.limits(transcript_records=1), java_transcript=path)
        self.assertEqual(report['status'], 'partial')
        self.assertEqual(len(report['java_transcript']['evidence']['records']), 1)
        self.assertTrue(report['java_transcript']['truncated'])
        report = collector.collect(self.root, self.limits(source_file_bytes=8), java_transcript=path)
        self.assertEqual(report['java_transcript']['status'], 'skipped')
        self.assertIsNone(report['java_transcript']['sha256'])

    def test_optional_sources_cannot_leave_root_or_follow_links(self):
        outside = self.base / 'outside-private.json'
        outside.write_text('SECRET_OUTSIDE_SOURCE')
        with mock.patch.object(collector, 'read_checked', side_effect=AssertionError('must not read')):
            report = collector.collect(self.root, probe_report=outside)
        self.assertEqual(report['status'], 'error')
        self.assertEqual(report['probe_report']['reason'], 'source_outside_root')
        linked = self.root / 'linked-private.json'
        self.link(outside, linked)
        report = collector.collect(self.root, probe_report=linked)
        self.assertEqual(report['probe_report']['status'], 'error')
        self.assertNotIn('SECRET_', json.dumps(report))

    def test_optional_missing_or_malformed_source_is_a_collection_error(self):
        report = collector.collect(self.root, probe_report='missing-private.json')
        self.assertEqual(report['status'], 'error')
        self.assertEqual(report['probe_report']['error_type'], 'FileNotFoundError')
        path = self.private_source('malformed-private.json', '{"SECRET_MALFORMED_JSON":')
        report = collector.collect(self.root, probe_report=path)
        self.assertEqual(report['status'], 'error')
        self.assertEqual(report['probe_report']['error_type'], 'JSONDecodeError')
        self.assertIsNotNone(report['probe_report']['sha256'])
        self.assertNotIn('SECRET_', json.dumps(report))

    def test_optional_cli_sources_are_sanitized_and_old_cli_still_works(self):
        probe = self.private_source('private-probe.json', json.dumps(self.probe_fixture()))
        transcript = self.private_source('private-java.txt', '{"kind":"fixture_cleanup","removed":false}\nSECRET_STDERR\n')
        output = self.base / 'report.json'
        result = subprocess.run([
            sys.executable, str(Path(collector.__file__)), '--root', str(self.root), '--output', str(output),
            '--probe-report', str(probe), '--java-transcript', str(transcript),
        ], capture_output=True, text=True, timeout=10)
        self.assertEqual(result.returncode, 0, result.stderr)
        report = json.loads(output.read_text())
        self.assertIn('probe_report', report)
        self.assertFalse(report['java_transcript']['evidence']['records'][0]['removed'])
        self.assertNotIn('SECRET_', result.stdout + result.stderr + output.read_text())

    def test_agent_transcript_preserves_typed_records_and_omits_private_payloads(self):
        expected = self.agent_fixture()
        records = [{**record, 'payload': {'uri': 'file:///SECRET_WORKSPACE/Main.java'},
                    'path': 'C:\\SECRET_USER\\fixture', 'env': {'TOKEN': 'SECRET_TOKEN'},
                    'error': 'SECRET_ERROR', 'stack': ['SECRET_STACK'], 'pid': 12345}
                   for record in expected]
        records.extend(({'kind': 'completion', 'result': 'SECRET_COMPLETION'},
                        {'kind': 'pass', 'windows_full_acceptance': True},
                        {'kind': ['SECRET_KIND']}, ['SECRET_ARRAY']))
        raw = ('\ufeff' + '\r\n'.join(json.dumps(record) for record in records)
               + '\r\nSECRET_STDERR\r\n').encode('utf-8')
        path = self.private_source('private-agent.txt', raw)
        report = collector.collect(self.root, agent_transcript=path)
        self.assertEqual(report['status'], 'complete')
        self.assertEqual(report['acceptance_result'], 'not_evaluated')
        source = report['agent_transcript']
        self.assertEqual(source['status'], 'collected')
        self.assertFalse(source['truncated'])
        self.assertEqual(source['bytes'], len(raw))
        self.assertEqual(source['sha256'], hashlib.sha256(raw).hexdigest())
        self.assertEqual(source['evidence']['records'], expected)
        self.assertEqual(source['evidence']['omitted_other_json_records'], 4)
        self.assertEqual(source['evidence']['omitted_non_json_lines'], 1)
        rendered = json.dumps(report)
        for secret in ('SECRET_', 'file:///', str(self.root), 'payload', 'windows_full_acceptance'):
            self.assertNotIn(secret, rendered)

    def test_agent_failure_and_false_witnesses_are_collected_without_runtime_acceptance(self):
        session, cleanup = self.agent_fixture()[0], self.agent_fixture()[-1]
        for record in (session, cleanup):
            for key, value in record.items():
                if type(value) is bool:
                    record[key] = False
        session['root_exit_code'] = None
        session['correction_change_result'] = 'not_attempted'
        cleanup.update(sessions_completed=0, primary_failed=True, cleanup_failed=True, failure_stage='stop')
        path = self.agent_source([session, cleanup])
        report = collector.collect(self.root, agent_transcript=path)
        self.assertEqual(report['status'], 'complete')
        self.assertEqual(report['agent_transcript']['evidence']['records'], [session, cleanup])
        self.assertEqual(report['acceptance_result'], 'not_evaluated')

    def test_agent_records_do_not_expand_direct_java_transcript_schema(self):
        agent = self.agent_source(self.agent_fixture() + [self.agent_diagnostics_fixture()]
                                  + self.agent_lifecycle_fixtures() + [self.agent_production_fixture(),
                                                                     self.agent_gc_control_fixture()])
        java = self.private_source('private-java.txt', '{"kind":"fixture_cleanup","removed":true}\n')
        report = collector.collect(self.root, java_transcript=agent, agent_transcript=java)
        self.assertEqual(report['status'], 'complete')
        self.assertEqual(report['java_transcript']['evidence']['records'], [])
        self.assertEqual(report['agent_transcript']['evidence']['records'], [])
        self.assertEqual(report['java_transcript']['evidence']['omitted_other_json_records'], 9)
        self.assertEqual(report['agent_transcript']['evidence']['omitted_other_json_records'], 1)

    def test_agent_session_numbers_require_bounded_integers(self):
        for kind, field, valid_values in (
                ('windows_java_session', 'session', (1, 2, 3)),
                ('windows_java_diagnostics', 'session', (1, 2, 3)),
                ('windows_java_cleanup', 'sessions_completed', (0, 1, 2, 3))):
            for value in valid_values + (-1, 0, 4, 2 ** 53, True, False, 1.0, None, '1', 'SECRET_SESSION'):
                valid = type(value) is int and value in valid_values
                with self.subTest(kind=kind, value=value):
                    path = self.agent_source([{'kind': kind, field: value}])
                    report = collector.collect(self.root, agent_transcript=path)
                    source = report['agent_transcript']
                    record = source['evidence']['records'][0]
                    if valid:
                        self.assertEqual(report['status'], 'complete')
                        self.assertEqual(record[field], value)
                    else:
                        self.assertEqual(report['status'], 'error')
                        self.assertNotIn(field, record)
                        self.assertIn('invalid_field_' + field, source['errors'])
                    self.assertNotIn('SECRET_', json.dumps(report))
        for kind in ('windows_java_session', 'windows_java_diagnostics'):
            with self.subTest(kind=kind, missing_session=True):
                path = self.agent_source([{'kind': kind}])
                report = collector.collect(self.root, agent_transcript=path)
                self.assertEqual(report['status'], 'error')
                self.assertIn('missing_session', report['agent_transcript']['errors'])

    def test_agent_count_and_exit_code_bounds_reject_wrong_types(self):
        for field, maximum, nullable in (('initialization_ms', 2 ** 53 - 1, False),
                                         ('shutdown_elapsed_ms', 2 ** 53 - 1, False),
                                         ('root_exit_code', 2 ** 32 - 1, True)):
            for value in (0, maximum, None, -1, maximum + 1, True, 0.0, 'SECRET_NUMBER'):
                valid = (value is None and nullable) or (type(value) is int and 0 <= value <= maximum)
                with self.subTest(field=field, value=value):
                    path = self.agent_source([{'kind': 'windows_java_session', 'session': 1, field: value}])
                    report = collector.collect(self.root, agent_transcript=path)
                    record = report['agent_transcript']['evidence']['records'][0]
                    self.assertEqual(report['status'], 'complete' if valid else 'error')
                    if valid:
                        self.assertEqual(record[field], value)
                    else:
                        self.assertNotIn(field, record)
                        self.assertIn('invalid_field_' + field, report['agent_transcript']['errors'])
                    self.assertNotIn('SECRET_', json.dumps(report))

    def test_agent_enum_and_boolean_fields_reject_untrusted_values(self):
        path = self.agent_source([
            {'kind': 'windows_java_session', 'session': 1, 'mode': 'SECRET_MODE',
             'semantic_checks_passed': 1, 'root_handle_signaled': 'SECRET_BOOLEAN',
             'correction_change_acknowledged': 'SECRET_ACKNOWLEDGED',
             'correction_change_result': 'SECRET_CHANGE_RESULT'},
            {'kind': 'windows_java_cleanup', 'sessions_completed': 0,
             'failure_stage': 'SECRET_STAGE', 'success': None, 'primary_failed': 0},
        ])
        report = collector.collect(self.root, agent_transcript=path)
        self.assertEqual(report['status'], 'error')
        self.assertEqual(set(report['agent_transcript']['errors']), {
            'invalid_field_mode', 'invalid_field_semantic_checks_passed',
            'invalid_field_root_handle_signaled', 'invalid_field_failure_stage',
            'invalid_field_success', 'invalid_field_primary_failed',
            'invalid_field_correction_change_acknowledged',
            'invalid_field_correction_change_result',
        })
        self.assertNotIn('SECRET_', json.dumps(report))
        stages = ('none', 'setup', 'initialize', 'open', 'diagnostics', 'hover', 'definition',
                  'completion', 'resolve', 'apply', 'undo', 'redo', 'sync', 'correction',
                  'close', 'stop', 'root_exit', 'agent_exit', 'fixture_cleanup')
        path = self.agent_source([{'kind': 'windows_java_cleanup', 'failure_stage': stage}
                                  for stage in stages])
        report = collector.collect(self.root, agent_transcript=path)
        self.assertEqual(report['status'], 'complete')
        self.assertEqual([record['failure_stage'] for record in report['agent_transcript']['evidence']['records']],
                         list(stages))

    def test_agent_diagnostics_keep_only_bounded_evidence_and_all_result_states(self):
        expected = [{**self.agent_diagnostics_fixture(), 'phase': phase, 'result': result,
                     'counters_saturated': phase == 'correction', 'elapsed_saturated': result == 'timeout'}
                    for phase in ('initial', 'correction')
                    for result in ('matched', 'timeout', 'request_error', 'malformed_events',
                                   'truncated', 'lagged', 'closed')]
        records = [{**record, 'uri': 'file:///SECRET_DIAGNOSTIC_PATH/Main.java',
                    'source': 'SECRET_SOURCE_BYTES', 'message': 'SECRET_DIAGNOSTIC_MESSAGE',
                    'errors': ['SECRET_REQUEST_ERROR'], 'stack': 'SECRET_STACK',
                    'payload': {'diagnostics': ['SECRET_RAW_DIAGNOSTIC']},
                    'environment': {'TOKEN': 'SECRET_ENV'}, 'SECRET_UNKNOWN_FIELD': True}
                   for record in expected]
        path = self.agent_source(records)
        raw = path.read_bytes()
        report = collector.collect(self.root, agent_transcript=path)
        self.assertEqual(report['status'], 'complete')
        self.assertEqual(report['acceptance_result'], 'not_evaluated')
        source = report['agent_transcript']
        self.assertEqual(source['status'], 'collected')
        self.assertFalse(source['truncated'])
        self.assertEqual(source['sha256'], hashlib.sha256(raw).hexdigest())
        self.assertEqual(source['bytes'], len(raw))
        self.assertEqual(source['evidence']['records'], expected)
        for secret in ('SECRET_', 'file:///', str(self.root), 'payload', 'environment'):
            self.assertNotIn(secret, json.dumps(report))

    def test_agent_diagnostic_counters_and_elapsed_time_require_bounded_integers(self):
        counters = [key for key, value in self.agent_diagnostics_fixture().items()
                    if type(value) is int and key not in ('session', 'elapsed_ms')]
        self.assertEqual(len(counters), 19)
        for field, maximum in [(field, 65535) for field in counters] + [('elapsed_ms', 300000)]:
            for value in (0, maximum, -1, maximum + 1, 2 ** 53, True, False, 1.0, None, '1', 'SECRET_COUNTER'):
                valid = type(value) is int and 0 <= value <= maximum
                with self.subTest(field=field, value=value):
                    record = {**self.agent_diagnostics_fixture(), field: value}
                    path = self.agent_source([record])
                    report = collector.collect(self.root, agent_transcript=path)
                    source = report['agent_transcript']
                    sanitized = source['evidence']['records'][0]
                    self.assertEqual(report['status'], 'complete' if valid else 'error')
                    if valid:
                        self.assertEqual(sanitized[field], value)
                    else:
                        self.assertNotIn(field, sanitized)
                        self.assertEqual(source['errors'], ['invalid_field_' + field])
                    self.assertNotIn('SECRET_', json.dumps(report))

    def test_agent_correction_change_result_preserves_only_known_outcomes(self):
        outcomes = ('not_attempted', 'request_error', 'acknowledgement_mismatch', 'acknowledged')
        for value in outcomes + ('SECRET_RESULT', 'matched', 1, True, None, ['acknowledged']):
            valid = isinstance(value, str) and value in outcomes
            with self.subTest(value=value):
                path = self.agent_source([{
                    'kind': 'windows_java_session', 'session': 2, 'correction_change_result': value,
                    'correction_change_acknowledged': value == 'acknowledged',
                }])
                report = collector.collect(self.root, agent_transcript=path)
                source = report['agent_transcript']
                record = source['evidence']['records'][0]
                self.assertEqual(report['status'], 'complete' if valid else 'error')
                if valid:
                    self.assertEqual(record['correction_change_result'], value)
                    self.assertEqual(record['correction_change_acknowledged'], value == 'acknowledged')
                else:
                    self.assertNotIn('correction_change_result', record)
                    self.assertEqual(source['errors'], ['invalid_field_correction_change_result'])
                self.assertNotIn('SECRET_', json.dumps(report))

    def test_correction_hover_is_bounded_private_and_cannot_replace_diagnostic_failure(self):
        original = {**self.agent_diagnostics_fixture(), 'session': 2,
                    'phase': 'correction', 'result': 'timeout', 'matching_batches': 0}
        for result in ('matched', 'no_match', 'request_error'):
            expected = {'kind': 'windows_java_correction_hover', 'session': 2,
                        'result': result, 'elapsed_ms': 60000, 'elapsed_saturated': False}
            private = {**expected, 'uri': 'file:///SECRET_PATH/Main.java',
                       'hover': {'contents': 'SECRET_HOVER'}, 'error': 'SECRET_ERROR',
                       'source': 'SECRET_SOURCE', 'success': True, 'correction_diagnostics': True}
            report = collector.collect(self.root, agent_transcript=self.agent_source([original, private]))
            self.assertEqual(report['status'], 'complete')
            self.assertEqual(report['acceptance_result'], 'not_evaluated')
            self.assertEqual(report['agent_transcript']['evidence']['records'], [original, expected])
            self.assertNotIn('SECRET_', json.dumps(report))

        fixture = {'kind': 'windows_java_correction_hover', 'session': 2,
                   'result': 'matched', 'elapsed_ms': 0, 'elapsed_saturated': False}
        invalid = {
            'session': (0, 4, True, 1.0, 'SECRET_SESSION'),
            'result': ('timeout', 'SECRET_RESULT', None, True, ['matched']),
            'elapsed_ms': (-1, 300001, True, 1.0, 'SECRET_TIME'),
            'elapsed_saturated': (0, 1, None, 'SECRET_BOOLEAN'),
        }
        for field, values in invalid.items():
            for value in values:
                with self.subTest(field=field, value=value):
                    report = collector.collect(self.root, agent_transcript=self.agent_source([
                        {**fixture, field: value}]))
                    self.assertEqual(report['status'], 'error')
                    source = report['agent_transcript']
                    errors = ['invalid_field_' + field]
                    if field == 'session':
                        errors.append('missing_session')
                    self.assertEqual(source['errors'], errors)
                    self.assertNotIn(field, source['evidence']['records'][0])
                    self.assertNotIn('SECRET_', json.dumps(report))
        maximum = {**fixture, 'elapsed_ms': 300000, 'elapsed_saturated': True}
        report = collector.collect(self.root, agent_transcript=self.agent_source([maximum]))
        self.assertEqual(report['status'], 'complete')
        self.assertEqual(report['agent_transcript']['evidence']['records'], [maximum])

    def test_agent_diagnostic_enums_and_saturation_are_strict(self):
        for field, invalid_values in (
                ('phase', ('SECRET_PHASE', 'fresh_data', 1, True, None, ['initial'])),
                ('result', ('SECRET_RESULT', 'success', 1, False, None, {'matched': True})),
                ('counters_saturated', ('SECRET_BOOLEAN', 'false', 0, 1, None, [])),
                ('elapsed_saturated', ('SECRET_BOOLEAN', 'false', 0, 1, None, []))):
            for value in invalid_values:
                with self.subTest(field=field, value=value):
                    path = self.agent_source([{**self.agent_diagnostics_fixture(), field: value}])
                    report = collector.collect(self.root, agent_transcript=path)
                    source = report['agent_transcript']
                    self.assertEqual(report['status'], 'error')
                    self.assertEqual(source['errors'], ['invalid_field_' + field])
                    self.assertNotIn(field, source['evidence']['records'][0])
                    self.assertNotIn('SECRET_', json.dumps(report))

    def test_correction_recovery_retains_original_timeout_and_scalar_only_evidence(self):
        original = {**self.agent_diagnostics_fixture(), 'session': 2, 'phase': 'correction',
                    'result': 'timeout', 'elapsed_ms': 60020, 'polls': 593,
                    'events': 0, 'matching_batches': 0}
        session = {**self.agent_fixture()[1], 'semantic_checks_passed': False,
                   'correction_diagnostics': False, 'correction_recovery_result': 'matched',
                   'correction_recovery_attempts': 1, 'correction_recovery_acknowledged': True,
                   'correction_recovery_witness': True, 'correction_recovery_unversioned': True,
                   'correction_recovery_budget_sufficient': True}
        cleanup = {**self.agent_fixture()[-1], 'spontaneous_success': False}
        for result in collector.CORRECTION_RECOVERY_RESULTS:
            expected = {**self.correction_recovery_fixture(), 'result': result}
            private = {**expected, 'source': 'SECRET_SOURCE', 'uri': 'file:///SECRET_PATH',
                       'environment': {'TOKEN': 'SECRET_TOKEN'}, 'payload': {'text': 'SECRET_TEXT'},
                       'message': 'SECRET_MESSAGE', 'raw_logs': 'SECRET_LOGS',
                       'success': True, 'correction_diagnostics': True}
            records = [original, private, session, cleanup]
            report = collector.collect(self.root, agent_transcript=self.agent_source(records))
            self.assertEqual(report['status'], 'complete')
            self.assertEqual(report['acceptance_result'], 'not_evaluated')
            self.assertEqual(report['agent_transcript']['evidence']['records'],
                             [original, expected, session, cleanup])
            self.assertNotIn('SECRET_', json.dumps(report))

    def test_correction_recovery_rejects_non_scalar_unknown_and_out_of_range_values(self):
        fixture = self.correction_recovery_fixture()
        bounds = {'session': (1, 3), 'attempts': (0, 1), 'available_budget_ms': (0, 360000),
                  'required_budget_ms': (0, 165000), 'request_timeout_ms': (0, 75000),
                  'witness_dispatch_window_ms': (0, 15000), 'event_poll_timeout_ms': (0, 75000),
                  'elapsed_ms': (0, 300000), 'polls': (0, 65535), 'events': (0, 65535)}
        invalid = {field: (low - 1, high + 1, True, False, 1.0, None, 'SECRET_VALUE', [1], {'value': 1})
                   for field, (low, high) in bounds.items()}
        invalid.update({field: (0, 1, None, 'SECRET_VALUE', [], {})
                        for field, value in fixture.items() if type(value) is bool})
        invalid.update({'result': ('SECRET_RESULT', 1, True, None, ['matched']),
                        'original_result': ('not_attempted', 'SECRET_ORIGINAL', True, None, ['timeout'])})
        for field, values in invalid.items():
            for value in values:
                with self.subTest(field=field, value=value):
                    report = collector.collect(self.root, agent_transcript=self.agent_source([
                        {**fixture, field: value}]))
                    source = report['agent_transcript']
                    self.assertEqual(report['status'], 'error')
                    self.assertNotIn(field, source['evidence']['records'][0])
                    self.assertIn('invalid_field_' + field, source['errors'])
                    self.assertNotIn('SECRET_', json.dumps(report))
        for field, (low, high) in bounds.items():
            for value in (low, high):
                expected = {**fixture, field: value}
                report = collector.collect(self.root, agent_transcript=self.agent_source([expected]))
                self.assertEqual(report['status'], 'complete')
                self.assertEqual(report['agent_transcript']['evidence']['records'], [expected])
        for field, values in {
            'correction_recovery_result': ('SECRET_RESULT', True, None, ['matched']),
            'correction_recovery_attempts': (-1, 2, True, 1.0, 'SECRET_COUNT'),
            'workflow_success': (0, 1, 'SECRET_BOOLEAN', None),
            'correction_recovery_witness': (0, 1, 'SECRET_BOOLEAN', None),
        }.items():
            for value in values:
                report = collector.collect(self.root, agent_transcript=self.agent_source([
                    {**self.agent_fixture()[1], field: value}]))
                self.assertEqual(report['status'], 'error')
                self.assertNotIn(field, report['agent_transcript']['evidence']['records'][0])
                self.assertNotIn('SECRET_', json.dumps(report))

    @unittest.skipUnless(shutil.which('pwsh'), 'PowerShell is required to execute the actual release predicate')
    def test_actual_powershell_editor_gate_accepts_only_the_complete_supported_workflow(self):
        spontaneous = self.agent_fixture() + [
            {**self.agent_diagnostics_fixture(), 'session': session, 'phase': phase}
            for session in (1, 2, 3) for phase in ('initial', 'correction')]
        recovered = json.loads(json.dumps(spontaneous))
        recovered[1].update(semantic_checks_passed=False, correction_diagnostics=False,
                            correction_recovery_result='matched', correction_recovery_attempts=1,
                            correction_recovery_acknowledged=True, correction_recovery_witness=True,
                            correction_recovery_unversioned=True, correction_recovery_budget_sufficient=True)
        recovered[3]['spontaneous_success'] = False
        recovered[7].update(result='timeout', matching_batches=0, elapsed_ms=60020)
        recovered.append(self.correction_recovery_fixture())
        cases = [{'name': 'spontaneous', 'records': spontaneous, 'count': 0, 'accept': True},
                 {'name': 'recovered', 'records': recovered, 'count': 1, 'accept': True}]
        def reject(name, index, field, value):
            records = json.loads(json.dumps(recovered))
            records[index][field] = value
            cases.append({'name': name, 'records': records, 'count': 0, 'accept': False})
        for field in ('exact_diagnostics', 'exact_definition', 'real_completion', 'deferred_import_resolve',
                      'primary_identity_unchanged', 'two_atomic_edits', 'advisory_command_skipped',
                      'actual_undo', 'actual_redo', 'versions_2_3_4_synced', 'correction_change_acknowledged',
                      'source_unchanged', 'root_observed_live', 'root_identity_verified', 'jdk_symbol_verified',
                      'shutdown_api_succeeded', 'root_handle_signaled', 'gracefully_exited', 'workflow_success'):
            reject('missing_' + field, 1, field, False)
        for index, field, value in (
            (1, 'correction_diagnostics', True), (1, 'semantic_checks_passed', True),
            (1, 'correction_recovery_unversioned', False), (3, 'spontaneous_success', True),
            (3, 'cleanup_failed', True), (3, 'synthetic_root_removed', False),
            (3, 'agent_exit_zero', False), (7, 'result', 'lagged'), (7, 'elapsed_ms', 59999),
            (6, 'result', 'timeout'), (-1, 'attempts', 2), (-1, 'acknowledged', False),
            (-1, 'witness', False), (-1, 'result', 'timeout'), (-1, 'budget_sufficient', False),
            (-1, 'available_budget_ms', 164999), (-1, 'required_budget_ms', 90000),
            (-1, 'elapsed_ms', 165000), (-1, 'cleanup_reserve_guaranteed', True),
            (-1, 'original_result', 'closed'), (-1, 'counters_saturated', True),
        ):
            reject(f'{index}_{field}', index, field, value)
        cases.extend([
            {'name': 'missing_recovery', 'records': recovered[:-1], 'accept': False, 'count': 0},
            {'name': 'duplicate_recovery', 'records': recovered + [recovered[-1]], 'accept': False, 'count': 0},
            {'name': 'unrequested_recovery', 'records': spontaneous + [recovered[-1]], 'accept': False, 'count': 0},
        ])
        cases_file = self.private_source('gate-cases.json', json.dumps(cases))
        harness = self.private_source('gate-check.ps1', r'''param($AcceptanceScript, $CasesFile)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$tokens = $null; $parseErrors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile($AcceptanceScript, [ref]$tokens, [ref]$parseErrors)
if ($parseErrors.Count -ne 0) { throw 'Acceptance script did not parse.' }
$function = $ast.Find({ param($node)
    $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -ceq 'Assert-AgentEditorReceipt'
}, $true)
if ($null -eq $function) { throw 'Missing actual editor acceptance function.' }
Invoke-Expression $function.Extent.Text
foreach ($case in (Get-Content -LiteralPath $CasesFile -Raw | ConvertFrom-Json)) {
    $accepted = $false; $count = -1
    try { $count = Assert-AgentEditorReceipt -Receipts @($case.records); $accepted = $true } catch {}
    if ($accepted -ne $case.accept -or ($accepted -and $count -ne $case.count)) {
        throw ('Unexpected editor verdict: ' + $case.name)
    }
}
''')
        result = subprocess.run([shutil.which('pwsh'), '-NoLogo', '-NoProfile', '-File', str(harness),
                                 '-AcceptanceScript', str(Path(__file__).with_name('windows_java_acceptance.ps1').resolve()),
                                 '-CasesFile', str(cases_file)], capture_output=True, text=True, timeout=60)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_agent_missing_malformed_and_read_failed_sources_keep_safe_errors(self):
        report = collector.collect(self.root, agent_transcript='missing-private-agent.txt')
        self.assertEqual(report['status'], 'error')
        self.assertEqual(report['agent_transcript']['error_type'], 'FileNotFoundError')
        path = self.private_source('private-agent.txt', '{"kind":"windows_java_cleanup","SECRET_BROKEN":')
        report = collector.collect(self.root, agent_transcript=path)
        self.assertEqual(report['status'], 'error')
        self.assertIn('malformed_json_line', report['agent_transcript']['errors'])
        self.assertIsNotNone(report['agent_transcript']['sha256'])
        self.assertNotIn('SECRET_', json.dumps(report))
        with mock.patch.object(collector, 'read_checked', side_effect=ValueError('SECRET_IDENTITY_CHANGED')):
            report = collector.collect(self.root, agent_transcript=path)
        self.assertEqual(report['status'], 'error')
        self.assertEqual(report['agent_transcript']['reason'], 'source_read_or_parse_failed')
        self.assertIsNone(report['agent_transcript']['sha256'])
        self.assertNotIn('SECRET_', json.dumps(report))

    def test_agent_lifecycle_records_keep_only_their_own_fields_and_preserve_failure(self):
        expected = self.agent_lifecycle_fixtures()
        failed = [{key: False if type(value) is bool else value for key, value in record.items()}
                  for record in self.agent_lifecycle_fixtures()]
        for record in failed:
            record.update(primary_failed=True, cleanup_failed=True, failure_stage='owner_death')
        failed[0].update(tasks_started=0, tasks_completed=0)
        failed[1].update(java_exit_code=None, task_exit_code=None)
        expected.extend(failed)
        records = [{**record, 'uri': 'file:///SECRET_LIFECYCLE/Main.java',
                    'path': 'C:\\SECRET_PRIVATE\\task.lock', 'error': 'SECRET_FAILURE',
                    'source': 'SECRET_SOURCE', 'payload': {'raw': 'SECRET_PAYLOAD'},
                    'stack': ['SECRET_STACK'], 'environment': {'TOKEN': 'SECRET_ENV'}}
                   for record in expected]
        # Even known fields from another receipt are ignored outside its schema.
        records[0].update(java_exit_code='SECRET_WRONG_SCHEMA', elapsed_ms='SECRET_ELAPSED')
        records[1].update(tasks_started='SECRET_WRONG_SCHEMA', mode='SECRET_MODE')
        path = self.agent_source(records)
        raw = path.read_bytes()
        report = collector.collect(self.root, agent_transcript=path)
        source = report['agent_transcript']
        self.assertEqual(report['status'], 'complete')
        self.assertEqual(report['acceptance_result'], 'not_evaluated')
        self.assertEqual(source['status'], 'collected')
        self.assertFalse(source['truncated'])
        self.assertEqual(source['sha256'], hashlib.sha256(raw).hexdigest())
        self.assertEqual(source['bytes'], len(raw))
        self.assertEqual(source['evidence']['records'], expected)
        for secret in ('SECRET_', 'file:///', str(self.root), 'payload', 'environment'):
            self.assertNotIn(secret, json.dumps(report))

    def test_agent_lifecycle_numbers_have_strict_bounds_and_nullable_exit_codes(self):
        concurrency, forced = self.agent_lifecycle_fixtures()
        for fixture, field, maximum, nullable in (
                (concurrency, 'tasks_started', 2, False),
                (concurrency, 'tasks_completed', 2, False),
                (forced, 'java_exit_code', 2 ** 32 - 1, True),
                (forced, 'task_exit_code', 2 ** 32 - 1, True),
                (forced, 'elapsed_ms', 300000, False)):
            for value in (0, 1, maximum, -1, maximum + 1, 2 ** 53, True, False, 1.0, None, 'SECRET_NUMBER'):
                valid = (value is None and nullable) or (type(value) is int and 0 <= value <= maximum)
                with self.subTest(kind=fixture['kind'], field=field, value=value):
                    path = self.agent_source([{**fixture, field: value}])
                    report = collector.collect(self.root, agent_transcript=path)
                    source = report['agent_transcript']
                    record = source['evidence']['records'][0]
                    self.assertEqual(report['status'], 'complete' if valid else 'error')
                    if valid:
                        self.assertEqual(record[field], value)
                    else:
                        self.assertNotIn(field, record)
                        self.assertEqual(source['errors'], ['invalid_field_' + field])
                    self.assertNotIn('SECRET_', json.dumps(report))

    def test_agent_lifecycle_boolean_fields_are_strict_and_preserve_false(self):
        for fixture in self.agent_lifecycle_fixtures():
            fields = [key for key, value in fixture.items() if type(value) is bool]
            for field in fields:
                for value in (True, False, 0, 1, 0.0, None, 'SECRET_BOOLEAN', []):
                    valid = type(value) is bool
                    with self.subTest(kind=fixture['kind'], field=field, value=value):
                        path = self.agent_source([{**fixture, field: value}])
                        report = collector.collect(self.root, agent_transcript=path)
                        source = report['agent_transcript']
                        record = source['evidence']['records'][0]
                        self.assertEqual(report['status'], 'complete' if valid else 'error')
                        if valid:
                            self.assertIs(record[field], value)
                        else:
                            self.assertNotIn(field, record)
                            self.assertEqual(source['errors'], ['invalid_field_' + field])
                        self.assertNotIn('SECRET_', json.dumps(report))

    def test_agent_lifecycle_failure_stages_accept_only_the_fixed_namespace(self):
        stages = ('none', 'setup', 'initialize', 'open', 'diagnostics', 'hover', 'task_start',
                  'task_identity', 'language_stop', 'task_survival', 'task_cancel', 'java_survival',
                  'owner_death', 'agent_exit', 'java_exit', 'task_exit', 'source', 'fixture_cleanup')
        for fixture in self.agent_lifecycle_fixtures():
            for stage in stages + ('SECRET_STAGE', 'root_exit', 'correction', 'stop', 0, True, None, []):
                valid = isinstance(stage, str) and stage in stages
                with self.subTest(kind=fixture['kind'], stage=stage):
                    path = self.agent_source([{**fixture, 'failure_stage': stage}])
                    report = collector.collect(self.root, agent_transcript=path)
                    source = report['agent_transcript']
                    record = source['evidence']['records'][0]
                    self.assertEqual(report['status'], 'complete' if valid else 'error')
                    if valid:
                        self.assertEqual(record['failure_stage'], stage)
                    else:
                        self.assertNotIn('failure_stage', record)
                        self.assertEqual(source['errors'], ['invalid_field_failure_stage'])
                    self.assertNotIn('SECRET_', json.dumps(report))
        # Adding lifecycle stages does not loosen the original cleanup record.
        path = self.agent_source([{'kind': 'windows_java_cleanup', 'failure_stage': 'owner_death'}])
        report = collector.collect(self.root, agent_transcript=path)
        self.assertEqual(report['status'], 'error')
        self.assertEqual(report['agent_transcript']['errors'], ['invalid_field_failure_stage'])

    def test_agent_production_preserves_failures_and_omits_all_private_fields(self):
        failed = {key: False if type(value) is bool else value
                  for key, value in self.agent_production_fixture().items()}
        failed.update(primary_failed=True, cleanup_failed=True, stop_status='error',
                      stop_reason='transport_failure', root_exit_code=None, failure_stage='stop')
        expected = [self.agent_production_fixture(), failed]
        records = [{**record, 'source': 'SECRET_SOURCE', 'uri': 'file:///SECRET_PRIVATE/Main.java',
                    'path': 'C:\\SECRET_PRIVATE\\fixture', 'error': 'SECRET_ERROR',
                    'messages': ['SECRET_PROTOCOL_MESSAGE'], 'stack': ['SECRET_STACK'],
                    'environment': {'TOKEN': 'SECRET_ENV'}, 'payload': {'raw': 'SECRET_PAYLOAD'},
                    'session': 'SECRET_OTHER_SCHEMA', 'java_exit_code': 'SECRET_OTHER_EXIT',
                    'tasks_started': 'SECRET_OTHER_COUNT'} for record in expected]
        path = self.agent_source(records)
        raw = path.read_bytes()
        report = collector.collect(self.root, agent_transcript=path)
        source = report['agent_transcript']
        self.assertEqual(report['status'], 'complete')
        self.assertEqual(report['acceptance_result'], 'not_evaluated')
        self.assertEqual(source['status'], 'collected')
        self.assertFalse(source['truncated'])
        self.assertEqual(source['sha256'], hashlib.sha256(raw).hexdigest())
        self.assertEqual(source['bytes'], len(raw))
        self.assertEqual(source['evidence']['records'], expected)
        for secret in ('SECRET_', 'file:///', str(self.root), 'payload', 'environment', 'messages'):
            self.assertNotIn(secret, json.dumps(report))

    def test_agent_production_booleans_require_actual_boolean_values(self):
        fixture = self.agent_production_fixture()
        fields = [key for key, value in fixture.items() if type(value) is bool]
        self.assertEqual(len(fields), 34)
        for field in fields:
            for value in (True, False, 0, 1, 0.0, None, 'SECRET_BOOLEAN', []):
                valid = type(value) is bool
                with self.subTest(field=field, value=value):
                    path = self.agent_source([{**fixture, field: value}])
                    report = collector.collect(self.root, agent_transcript=path)
                    source = report['agent_transcript']
                    record = source['evidence']['records'][0]
                    self.assertEqual(report['status'], 'complete' if valid else 'error')
                    if valid:
                        self.assertIs(record[field], value)
                    else:
                        self.assertNotIn(field, record)
                        self.assertEqual(source['errors'], ['invalid_field_' + field])
                    self.assertNotIn('SECRET_', json.dumps(report))

    def test_idle_receipt_keeps_separate_spontaneous_recovered_and_failed_verdicts(self):
        failed = {**self.agent_idle_fixture(True), 'success': False, 'workflow_success': False,
                  'primary_failed': True, 'failure_stage': 'correction',
                  'recovery_result': 'timeout', 'recovery_witness': False}
        for expected in (self.agent_idle_fixture(), self.agent_idle_fixture(True), failed):
            with self.subTest(result=expected['recovery_result']):
                raw = {**expected, 'source': 'SECRET_SOURCE', 'uri': 'file:///SECRET_ROOT/Main.java',
                       'path': 'C:\\SECRET_ROOT', 'environment': {'TOKEN': 'SECRET_ENV'},
                       'raw_output': 'SECRET_VM_LOG', 'response': {'raw': 'SECRET_JDT'},
                       'diagnostics': ['SECRET_MESSAGE'], 'pid': 314}
                report = collector.collect(self.root, agent_transcript=self.agent_source([raw]))
                self.assertEqual(report['status'], 'complete')
                self.assertEqual(report['acceptance_result'], 'not_evaluated')
                self.assertEqual(report['agent_transcript']['evidence']['records'], [expected])
                for private in ('SECRET_', 'file:///', 'raw_output', 'environment', 'pid'):
                    self.assertNotIn(private, json.dumps(report))
                result, errors, truncated = collector.sanitize_transcript(
                    json.dumps(raw).encode(), collector.LIMITS)
                self.assertEqual(result['records'], [])
                self.assertFalse(errors)
                self.assertFalse(truncated)

    def test_idle_receipt_requires_every_typed_scalar_and_exact_fixed_budgets(self):
        fixture = self.agent_idle_fixture(True)
        schema = collector.AGENT_TRANSCRIPT_FIELDS['windows_java_idle_correction']
        self.assertEqual(set(fixture), {'kind', *schema})
        for field, kind in schema.items():
            missing = dict(fixture)
            del missing[field]
            _, errors, _ = collector.sanitize_agent_transcript(json.dumps(missing).encode(), collector.LIMITS)
            self.assertIn('missing_field_' + field, errors)
            invalids = [None, [], [fixture[field]], {}, 'SECRET_INVALID']
            if kind == 'bool':
                invalids += [0, 1, 0.0]
            elif isinstance(kind, tuple) and kind[0] == 'integer_range':
                invalids += [True, False, 1.0, kind[1] - 1, kind[2] + 1]
            elif kind == '?u32':
                invalids.remove(None)
                invalids += [True, False, -1, 2 ** 32, 1.0]
            else:
                invalids += [True, False, 0, 1.0]
            for invalid in invalids:
                with self.subTest(field=field, invalid=invalid):
                    result, errors, _ = collector.sanitize_agent_transcript(
                        json.dumps({**fixture, field: invalid}).encode(), collector.LIMITS)
                    self.assertIn('invalid_field_' + field, errors)
                    self.assertNotIn(field, result['records'][0])
                    self.assertNotIn('SECRET_', json.dumps(result))

    def test_idle_receipt_duplicate_records_and_json_fields_fail_closed(self):
        fixture = self.agent_idle_fixture()
        data = json.dumps(fixture)
        _, errors, _ = collector.sanitize_agent_transcript((data + '\n' + data).encode(), collector.LIMITS)
        self.assertEqual(errors, {'duplicate_idle_receipt'})
        for extra in ('"success":false', '"success":true', '"kind":"windows_java_idle_correction"',
                      '"SECRET_UNKNOWN":"SECRET_VALUE","SECRET_UNKNOWN":true'):
            with self.subTest(extra=extra):
                duplicate = data[:-1] + ',' + extra + '}'
                result, errors, _ = collector.sanitize_agent_transcript(duplicate.encode(), collector.LIMITS)
                self.assertEqual(result['records'], [])
                self.assertEqual(errors, {'duplicate_idle_receipt_field'})
                self.assertNotIn('SECRET_', json.dumps(result))

    @unittest.skipUnless(shutil.which('pwsh'), 'PowerShell is required for the actual required-idle predicate')
    def test_actual_idle_powershell_gate_rejects_missing_forged_and_late_witnesses(self):
        spontaneous = self.agent_idle_fixture()
        recovered = self.agent_idle_fixture(True)
        cases = [{'name': 'spontaneous', 'records': [spontaneous], 'accept': True},
                 {'name': 'recovered_unversioned', 'records': [recovered], 'accept': True},
                 {'name': 'recovered_versioned', 'records': [{**recovered, 'recovery_unversioned': False}], 'accept': True},
                 {'name': 'zero_tests', 'records': [], 'accept': False},
                 {'name': 'scalar_receipt_root', 'records': recovered, 'accept': False},
                 {'name': 'null_receipt_root', 'records': None, 'accept': False},
                 {'name': 'string_receipt_root', 'records': 'SECRET_ROOT', 'accept': False},
                 {'name': 'number_receipt_root', 'records': 7, 'accept': False},
                 {'name': 'duplicate_receipts', 'records': [spontaneous, recovered], 'accept': False},
                 {'name': 'array_receipt', 'records': [[recovered]], 'accept': False},
                 {'name': 'array_duplicate_receipts', 'records': [[spontaneous, recovered]], 'accept': False},
                 {'name': 'deeper_array_receipt', 'records': [[[recovered]]], 'accept': False},
                 {'name': 'array_before_scalar_receipt', 'records': [[recovered], spontaneous], 'accept': False},
                 {'name': 'array_after_scalar_receipt', 'records': [spontaneous, [recovered]], 'accept': False},
                 {'name': 'empty_array_with_valid_receipt', 'records': [[], recovered], 'accept': False},
                 {'name': 'unrelated_array_with_valid_receipt', 'records': [[self.agent_production_fixture()], recovered], 'accept': False},
                 {'name': 'null_with_valid_receipt', 'records': [None, recovered], 'accept': False},
                 {'name': 'string_with_valid_receipt', 'records': ['SECRET_SCALAR', recovered], 'accept': False},
                 {'name': 'number_with_valid_receipt', 'records': [7, recovered], 'accept': False},
                 {'name': 'unrelated_quick', 'records': [self.agent_production_fixture()], 'accept': False},
                 {'name': 'independent_quick_and_idle', 'records': [self.agent_production_fixture(), recovered], 'accept': True}]
        for fixture in (spontaneous, recovered):
            label = fixture['spontaneous_result'] + '_'
            forced = {**fixture, 'stop_status': 'forced', 'stop_reason': 'grace_expired',
                      'root_exit_code': 1067, 'shutdown_response_received': False, 'exit_frame_completed': False}
            cases.append({'name': label + 'honest_owned_forced_stop', 'records': [forced], 'accept': True})
            for field, original in fixture.items():
                missing = dict(fixture)
                del missing[field]
                cases.append({'name': label + 'missing_' + field, 'records': [missing], 'accept': False})
                invalids = [None, [], [original], {}, 'SECRET_SCALAR']
                if type(original) is bool:
                    invalids += [0, 1, 0.0]
                    if field != 'recovery_unversioned' or not fixture['recovery_witness']:
                        invalids += [not original]
                elif type(original) is int:
                    invalids += [-1, True, False, float(original), 2 ** 53]
                    if field in collector.IDLE_FIXED_BUDGETS:
                        invalids += [original - 1, original + 1]
                for invalid in invalids:
                    cases.append({'name': label + field + '_' + repr(invalid),
                                  'records': [{**fixture, field: invalid}], 'accept': False})
        def reject(name, fixture=recovered, **changes):
            cases.append({'name': name, 'records': [{**fixture, **changes}], 'accept': False})
        for result in collector.CORRECTION_RECOVERY_RESULTS:
            if result != 'matched':
                reject('failed_recovery_' + result, recovery_result=result)
        for result in ('request_error', 'malformed_events', 'truncated', 'lagged', 'closed'):
            reject('ineligible_' + result, spontaneous_result=result)
        reject('insufficient_full_recovery_plus_close', recovery_available_budget_ms=239999)
        reject('budget_before_required_idle_and_timeout', recovery_available_budget_ms=270001)
        reject('overlarge_available_budget', recovery_available_budget_ms=360001)
        reject('repeated_refresh', recovery_attempts=2)
        reject('forged_spontaneous_batch', spontaneous_matching_batches=1)
        reject('forged_spontaneous_success', spontaneous_success=True, correction_diagnostics=True)
        reject('late_primary', primary_elapsed_ms=360000, cleanup_started_ms=360000, elapsed_ms=360001)
        reject('late_outer', elapsed_ms=480000)
        reject('cleanup_started_before_primary_finished', cleanup_started_ms=119999)
        reject('elapsed_before_cleanup', elapsed_ms=119999)
        reject('impossible_recovery_admission_time', recovery_available_budget_ms=240000,
               primary_elapsed_ms=100000, cleanup_started_ms=100000)
        reject('timeout_before_30_idle_plus_60_dispatch', recovery_available_budget_ms=280000,
               primary_elapsed_ms=89999, cleanup_started_ms=89999)
        reject('no_spontaneous_witness', spontaneous, spontaneous_matching_batches=0)
        reject('no_initial_idle', spontaneous, primary_elapsed_ms=29999, cleanup_started_ms=29999)
        reject('unexpected_refresh', spontaneous, recovery_attempts=1)
        reject('unrequested_recovery_budget', spontaneous, recovery_available_budget_ms=240000)
        reject('wrong_route', route='diagnostic_agent_normal_client')
        reject('forced_stop_with_graceful_reason', stop_status='forced')
        reject('graceful_nonzero_exit', root_exit_code=1)
        reject('noncompleted_stop', stop_status='error', stop_reason='transport_failure')
        for case in cases:
            case['array_root'] = isinstance(case['records'], list)
            case['top_level_count'] = len(case['records']) if case['array_root'] else 0
            case['nested_arrays'] = (sum(isinstance(record, list) for record in case['records'])
                                     if case['array_root'] else 0)
            if case['accept']:
                idle = next(record for record in case['records']
                            if record['kind'] == 'windows_java_idle_correction')
                case['count'] = int(idle['spontaneous_result'] == 'timeout')
        data = self.private_source('idle-gate-cases.json', json.dumps(cases))
        harness = self.private_source('idle-gate-check.ps1', r'''param($Predicate, $Cases)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
. $Predicate
function Assert-TestArgumentShape([object] $Receipts, [bool] $ArrayRoot, [int] $ExpectedCount, [int] $ExpectedArrays) {
    # Use the production parameter type and the identical call-site expression.
    # This proves that the caller/binder retains both root and element shapes.
    if (($Receipts -is [array]) -ne $ArrayRoot) { throw 'Test call changed receipt root shape.' }
    if (-not $ArrayRoot) { return }
    if ($Receipts.Length -ne $ExpectedCount) { throw 'Test call changed top-level receipt count.' }
    $arrays = 0
    for ($index = 0; $index -lt $Receipts.Length; $index++) {
        if ($Receipts[$index] -is [array]) { $arrays++ }
    }
    if ($arrays -ne $ExpectedArrays) { throw 'Test call flattened a nested receipt array.' }
}
foreach ($case in (Get-Content -LiteralPath $Cases -Raw | ConvertFrom-Json -Depth 10)) {
    Assert-TestArgumentShape -Receipts $case.records -ArrayRoot $case.array_root -ExpectedCount $case.top_level_count -ExpectedArrays $case.nested_arrays
    $accepted = $false; $count = -1
    try { $count = Assert-IdleCorrectionReceipt -Receipts $case.records; $accepted = $true } catch {}
    if ($accepted -ne $case.accept -or ($accepted -and $count -ne $case.count)) {
        throw ('Unexpected idle verdict: ' + $case.name)
    }
}
''')
        result = subprocess.run([shutil.which('pwsh'), '-NoLogo', '-NoProfile', '-File', str(harness),
                                 '-Predicate', str(Path(__file__).with_name('java_idle_acceptance_predicate.ps1').resolve()),
                                 '-Cases', str(data)], capture_output=True, text=True, timeout=60)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_workspace_type_receipt_preserves_failure_without_private_result_data(self):
        good = self.agent_workspace_type_fixture()
        failed = {**good, 'success': False, 'primary_failed': True, 'failure_stage': 'query',
                  'exact_type_name': False, 'exact_type_uri': False, 'exact_declaration_range': False,
                  'actual_frontend_navigation': False, 'dirty_buffer_reused': False,
                  'undo_redo_preserved': False}
        expected = [good, failed]
        path = self.agent_source([{**record, 'query': 'SECRET_QUERY', 'name': 'SECRET_TYPE',
            'uri': 'file:///SECRET_ROOT/Source.java', 'path': 'C:\\SECRET_ROOT',
            'source': 'SECRET_SOURCE', 'draft': 'SECRET_DIRTY_TEXT',
            'revision': 'SECRET_REVISION', 'response': {'result': 'SECRET_PROTOCOL'},
            'error': 'SECRET_ERROR', 'raw_output': 'SECRET_PRIVATE_LOG', 'pid': 314,
        } for record in expected])
        report = collector.collect(self.root, agent_transcript=path)
        self.assertEqual(report['status'], 'complete')
        self.assertEqual(report['acceptance_result'], 'not_evaluated')
        self.assertEqual(report['agent_transcript']['evidence']['records'], expected)
        for private in ('SECRET_', 'file:///', str(self.root), 'raw_output', 'revision', 'pid'):
            self.assertNotIn(private, json.dumps(report))
        other, errors, truncation = collector.sanitize_transcript(path.read_bytes(), collector.LIMITS)
        self.assertEqual(other['records'], [])
        self.assertEqual(other['omitted_other_json_records'], 2)
        self.assertEqual(errors, set())
        self.assertEqual(truncation, set())

    def test_workspace_type_receipt_enforces_boolean_scalar_and_counter_types(self):
        fixture = self.agent_workspace_type_fixture()
        fields = [key for key, value in fixture.items() if type(value) is bool]
        self.assertEqual(len(fields), 21)
        for field in fields:
            for value in (True, False, 0, 1, 0.0, None, 'SECRET_BOOLEAN', [], {}):
                with self.subTest(field=field, value=value):
                    report = collector.collect(self.root, agent_transcript=self.agent_source([
                        {**fixture, field: value}]))
                    source = report['agent_transcript']
                    record = source['evidence']['records'][0]
                    valid = type(value) is bool
                    self.assertEqual(report['status'], 'complete' if valid else 'error')
                    if valid:
                        self.assertIs(record[field], value)
                    else:
                        self.assertNotIn(field, record)
                        self.assertEqual(source['errors'], ['invalid_field_' + field])
                    self.assertNotIn('SECRET_', json.dumps(report))
        for value in (0, 240000, -1, 240001, True, 1.0, None, 'SECRET_COUNT'):
            with self.subTest(elapsed_ms=value):
                report = collector.collect(self.root, agent_transcript=self.agent_source([
                    {**fixture, 'elapsed_ms': value}]))
                valid = type(value) is int and 0 <= value <= 240000
                self.assertEqual(report['status'], 'complete' if valid else 'error')
                self.assertNotIn('SECRET_', json.dumps(report))
        stages = ('none', 'setup', 'support', 'query', 'negative_query', 'resolve', 'read', 'frontend')
        for stage in (*stages, 'SECRET_STAGE', 'owner_death', 0, None, []):
            with self.subTest(stage=stage):
                report = collector.collect(self.root, agent_transcript=self.agent_source([
                    {**fixture, 'failure_stage': stage}]))
                valid = isinstance(stage, str) and stage in stages
                self.assertEqual(report['status'], 'complete' if valid else 'error')
                self.assertNotIn('SECRET_', json.dumps(report))

    @unittest.skipUnless(shutil.which('pwsh'), 'PowerShell is required for the actual workspace type predicate')
    def test_actual_workspace_type_predicate_rejects_missing_and_forged_witnesses(self):
        good = self.agent_workspace_type_fixture()
        cases = [{'name': 'complete', 'records': [good], 'accept': True},
                 {'name': 'zero_tests', 'records': [], 'accept': False},
                 {'name': 'duplicate_receipts', 'records': [good, good], 'accept': False},
                 {'name': 'wrong_kind', 'records': [{**good, 'kind': 'windows_java_production'}], 'accept': False}]
        for field, original in good.items():
            missing = dict(good)
            del missing[field]
            cases.append({'name': 'missing_' + field, 'records': [missing], 'accept': False})
            if type(original) is bool:
                invalids = (not original, 0, 1, None, 'true', [], [original], {})
            elif field == 'elapsed_ms':
                invalids = (-1, 240001, True, 1.5, None, '1234', [], [1234])
            else:
                invalids = (None, 0, [], [original], 'SECRET_ENUM')
            for invalid in invalids:
                cases.append({'name': field + '_' + repr(invalid),
                              'records': [{**good, field: invalid}], 'accept': False})
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
    $accepted = $false
    try { Assert-WorkspaceTypeReceipt -Receipts @($case.records); $accepted = $true } catch {}
    if ($accepted -ne $case.accept) { throw ('Unexpected workspace type verdict: ' + $case.name) }
}
''', encoding='utf-8')
            result = subprocess.run([shutil.which('pwsh'), '-NoLogo', '-NoProfile', '-File', str(script),
                                     '-Predicate', str(Path(__file__).with_name('java_workspace_type_acceptance_predicate.ps1').resolve()),
                                     '-Cases', str(data)], capture_output=True, text=True, timeout=60)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_organize_receipts_preserve_failures_without_source_protocol_or_candidate_data(self):
        failed = {**self.agent_organize_fixture(), 'success': False, 'primary_failed': True,
                  'cleanup_failed': True, 'failure_stage': 'ambiguity',
                  'ambiguous_candidates_skipped': False, 'independent_import_added': False,
                  'ambiguity_edit_count': 0, 'synthetic_root_removed': False}
        expected = [self.agent_organize_fixture(), failed]
        path = self.agent_source([{**record,
            'source': 'SECRET_JAVA_SOURCE', 'draft': 'SECRET_UNSAVED_DRAFT',
            'edits': [{'newText': 'SECRET_EDIT_TEXT'}], 'candidate': 'SECRET_PROJECT_TYPE',
            'uri': 'file:///SECRET_PRIVATE/Fixture.java', 'path': 'C:\\SECRET_ROOT',
            'error': 'SECRET_ERROR', 'response': {'raw': 'SECRET_JDT_PROTOCOL'},
            'raw_log': 'SECRET_PRIVATE_LOG', 'route': 'SECRET_OTHER_SCHEMA',
            'pid': 314, 'environment': {'TOKEN': 'SECRET_ENV'},
        } for record in expected])
        report = collector.collect(self.root, agent_transcript=path)
        self.assertEqual(report['status'], 'complete')
        self.assertEqual(report['acceptance_result'], 'not_evaluated')
        self.assertEqual(report['agent_transcript']['evidence']['records'], expected)
        for private in ('SECRET_', 'file:///', str(self.root), 'newText', 'raw_log', 'pid'):
            self.assertNotIn(private, json.dumps(report))
        # The other transcript schema must not gain this acceptance namespace.
        other, errors, truncation = collector.sanitize_transcript(path.read_bytes(), collector.LIMITS)
        self.assertEqual(other['records'], [])
        self.assertEqual(other['omitted_other_json_records'], 2)
        self.assertEqual(errors, set())
        self.assertEqual(truncation, set())

    def test_organize_receipt_booleans_require_real_boolean_values(self):
        fixture = self.agent_organize_fixture()
        fields = [key for key, value in fixture.items() if type(value) is bool]
        self.assertEqual(len(fields), 26)
        for field in fields:
            for value in (True, False, 0, 1, None, 'SECRET_BOOLEAN', [], {}):
                with self.subTest(field=field, value=value):
                    report = collector.collect(self.root, agent_transcript=self.agent_source([
                        {**fixture, field: value}]))
                    source = report['agent_transcript']
                    record = source['evidence']['records'][0]
                    valid = type(value) is bool
                    self.assertEqual(report['status'], 'complete' if valid else 'error')
                    if valid:
                        self.assertIs(record[field], value)
                    else:
                        self.assertNotIn(field, record)
                        self.assertEqual(source['errors'], ['invalid_field_' + field])
                    self.assertNotIn('SECRET_', json.dumps(report))

    def test_organize_receipt_counts_are_bounded_and_enums_are_closed(self):
        fixture = self.agent_organize_fixture()
        for field, maximum in [('main_edit_count', 1024), ('ambiguity_edit_count', 1024),
                               ('observed_editor_stages', 5), ('elapsed_ms', 240000)]:
            for value in (0, maximum, -1, maximum + 1, True, 1.0, None, 'SECRET_COUNT'):
                with self.subTest(field=field, value=value):
                    report = collector.collect(self.root, agent_transcript=self.agent_source([
                        {**fixture, field: value}]))
                    source = report['agent_transcript']
                    record = source['evidence']['records'][0]
                    valid = type(value) is int and 0 <= value <= maximum
                    self.assertEqual(report['status'], 'complete' if valid else 'error')
                    if valid:
                        self.assertEqual(record[field], value)
                    else:
                        self.assertNotIn(field, record)
                        self.assertEqual(source['errors'], ['invalid_field_' + field])
                    self.assertNotIn('SECRET_', json.dumps(report))
        stages = ('none', 'setup', 'support', 'index_witness', 'unsaved_sync', 'organize',
                  'preview', 'cancel', 'apply', 'undo', 'redo', 'ambiguity', 'close')
        for stage in (*stages, 'SECRET_STAGE', 'owner_death', 0, None, []):
            with self.subTest(stage=stage):
                report = collector.collect(self.root, agent_transcript=self.agent_source([
                    {**fixture, 'failure_stage': stage}]))
                source = report['agent_transcript']
                valid = isinstance(stage, str) and stage in stages
                self.assertEqual(report['status'], 'complete' if valid else 'error')
                if valid:
                    self.assertEqual(source['evidence']['records'][0]['failure_stage'], stage)
                else:
                    self.assertNotIn('failure_stage', source['evidence']['records'][0])
                    self.assertEqual(source['errors'], ['invalid_field_failure_stage'])
                self.assertNotIn('SECRET_', json.dumps(report))

    def test_gc_control_receipt_is_distinct_and_keeps_forced_shutdown_a_failure(self):
        natural = self.agent_gc_control_fixture()
        forced = {**natural, 'success': False, 'cleanup_failed': True,
                  'stop_status': 'forced', 'stop_reason': 'grace_expired',
                  'root_exit_code': 1, 'failure_stage': 'root_exit'}
        records = [{**record, 'pid': 314, 'creation_time_100ns_since_1601': 133700000000000000,
                    'selection_path': 'C:\\SECRET_PRIVATE\\selection.json',
                    'log_path': 'C:\\SECRET_PRIVATE\\cedar-gc-314.log',
                    'raw_log': 'SECRET_LOG', 'heap': 'SECRET_RAW_HEAP',
                    'root_image': 'C:\\SECRET_PRIVATE\\java.exe'} for record in (natural, forced)]
        selection = {'kind': 'cedar_gc_control_selection', 'pid': 314,
                     'selection_path': 'SECRET_PRIVATE_SELECTION'}
        path = self.agent_source(records + [selection])
        report = collector.collect(self.root, agent_transcript=path)
        evidence = report['agent_transcript']['evidence']
        self.assertEqual(report['status'], 'complete')
        self.assertEqual(report['acceptance_result'], 'not_evaluated')
        self.assertEqual(evidence['records'], [natural, forced])
        self.assertEqual(evidence['omitted_other_json_records'], 1)
        for forbidden in ('SECRET_', 'pid', 'creation_time_100ns_since_1601', 'selection_path',
                          'root_image', 'raw_log', 'log_path', 'heap', 'normal_agent_client'):
            self.assertNotIn(forbidden, json.dumps(report))

    def test_gc_control_and_shipping_routes_cannot_be_mixed(self):
        for fixture, good, bad in (
                (self.agent_production_fixture(), 'normal_agent_client', 'diagnostic_agent_normal_client'),
                (self.agent_gc_control_fixture(), 'diagnostic_agent_normal_client', 'normal_agent_client')):
            for route in (good, bad, 'SECRET_ROUTE', None, True, 1, []):
                with self.subTest(kind=fixture['kind'], route=route):
                    path = self.agent_source([{**fixture, 'route': route}])
                    report = collector.collect(self.root, agent_transcript=path)
                    source = report['agent_transcript']
                    self.assertEqual(report['status'], 'complete' if route == good else 'error')
                    if route == good:
                        self.assertEqual(source['evidence']['records'][0], fixture)
                    else:
                        self.assertNotIn('route', source['evidence']['records'][0])
                        self.assertEqual(source['errors'], ['invalid_field_route'])
                    self.assertNotIn('SECRET_', json.dumps(report))
        missing = self.agent_gc_control_fixture()
        del missing['route']
        report = collector.collect(self.root, agent_transcript=self.agent_source([missing]))
        self.assertEqual(report['status'], 'error')
        self.assertEqual(report['agent_transcript']['errors'], ['missing_gc_control_route'])

    def test_gc_control_receipt_retains_exact_field_types_and_bounds(self):
        fixture = self.agent_gc_control_fixture()
        for field, value in fixture.items():
            if type(value) is bool:
                for invalid in (0, 1, None, 'SECRET_BOOL'):
                    with self.subTest(field=field, value=invalid):
                        report = collector.collect(self.root, agent_transcript=self.agent_source([
                            {**fixture, field: invalid}]))
                        self.assertEqual(report['status'], 'error')
                        self.assertNotIn(field, report['agent_transcript']['evidence']['records'][0])
                        self.assertEqual(report['agent_transcript']['errors'], ['invalid_field_' + field])
        for field, invalid in (('root_exit_code', 2 ** 32), ('root_exit_code', True),
                               ('elapsed_ms', 300001), ('elapsed_ms', -1),
                               ('failure_stage', 'owner_death'), ('stop_status', 'SECRET_STATUS'),
                               ('stop_reason', 'SECRET_REASON')):
            with self.subTest(field=field, value=invalid):
                report = collector.collect(self.root, agent_transcript=self.agent_source([
                    {**fixture, field: invalid}]))
                self.assertEqual(report['status'], 'error')
                self.assertNotIn(field, report['agent_transcript']['evidence']['records'][0])
                self.assertEqual(report['agent_transcript']['errors'], ['invalid_field_' + field])

    def test_agent_production_exit_code_and_elapsed_time_have_exact_bounds(self):
        for field, maximum, nullable in (('root_exit_code', 2 ** 32 - 1, True),
                                         ('elapsed_ms', 300000, False)):
            for value in (0, maximum, -1, maximum + 1, 2 ** 53, True, False, 1.0, None, 'SECRET_NUMBER'):
                valid = (value is None and nullable) or (type(value) is int and 0 <= value <= maximum)
                with self.subTest(field=field, value=value):
                    path = self.agent_source([{**self.agent_production_fixture(), field: value}])
                    report = collector.collect(self.root, agent_transcript=path)
                    source = report['agent_transcript']
                    record = source['evidence']['records'][0]
                    self.assertEqual(report['status'], 'complete' if valid else 'error')
                    if valid:
                        self.assertEqual(record[field], value)
                    else:
                        self.assertNotIn(field, record)
                        self.assertEqual(source['errors'], ['invalid_field_' + field])
                    self.assertNotIn('SECRET_', json.dumps(report))

    def test_agent_production_enum_namespaces_are_fixed_and_do_not_accept_ownership_stages(self):
        enums = {
            'route': ('normal_agent_client',),
            'stop_status': ('not_attempted', 'graceful', 'forced', 'error'),
            'stop_reason': ('not_attempted', 'root_exited', 'grace_expired', 'aborted',
                            'transport_failure', 'worker_panicked'),
            'failure_stage': ('none', 'setup', 'initialize', 'open', 'diagnostics', 'hover',
                              'definition', 'completion', 'resolve', 'apply', 'undo', 'redo',
                              'sync', 'correction', 'close', 'stop', 'root_exit', 'agent_exit',
                              'fixture_cleanup'),
        }
        for field, allowed in enums.items():
            for value in allowed + ('SECRET_ENUM', 'owner_death', 'task_start', 'java_exit',
                                    '', 0, True, None, ['normal_agent_client']):
                valid = isinstance(value, str) and value in allowed
                with self.subTest(field=field, value=value):
                    path = self.agent_source([{**self.agent_production_fixture(), field: value}])
                    report = collector.collect(self.root, agent_transcript=path)
                    source = report['agent_transcript']
                    record = source['evidence']['records'][0]
                    self.assertEqual(report['status'], 'complete' if valid else 'error')
                    if valid:
                        self.assertEqual(record[field], value)
                    else:
                        self.assertNotIn(field, record)
                        self.assertEqual(source['errors'], ['invalid_field_' + field])
                    self.assertNotIn('SECRET_', json.dumps(report))

    def test_agent_source_cannot_leave_root_or_follow_file_and_directory_links(self):
        outside = self.base / 'outside-private-agent.txt'
        outside.write_text('SECRET_OUTSIDE_SOURCE')
        for path in (outside, Path('../outside-private-agent.txt')):
            with self.subTest(path=path), \
                    mock.patch.object(collector, 'read_checked', side_effect=AssertionError('must not read')):
                report = collector.collect(self.root, agent_transcript=path)
                self.assertEqual(report['status'], 'error')
                self.assertEqual(report['agent_transcript']['reason'], 'source_outside_root')
                self.assertNotIn(str(self.base), json.dumps(report))
        linked_file = self.root / 'linked-private.txt'
        self.link(outside, linked_file)
        linked_directory = self.root / 'linked-directory'
        self.link(self.base, linked_directory, directory=True)
        for path in (linked_file, linked_directory / outside.name):
            with self.subTest(path=path), \
                    mock.patch.object(collector, 'read_checked', side_effect=AssertionError('must not read')):
                report = collector.collect(self.root, agent_transcript=path)
                self.assertEqual(report['agent_transcript']['status'], 'error')
                self.assertIsNone(report['agent_transcript']['sha256'])
                self.assertNotIn('SECRET_', json.dumps(report))

    def test_agent_source_rejects_hard_links_without_reading(self):
        outside = self.base / 'outside-private-agent.txt'
        outside.write_text('SECRET_HARDLINK_SOURCE')
        path = self.root / 'private-agent.txt'
        try:
            os.link(outside, path)
        except OSError as error:
            self.skipTest('Runner does not permit hardlinks: ' + type(error).__name__)
        with mock.patch.object(collector, 'read_checked', side_effect=AssertionError('must not read')):
            report = collector.collect(self.root, agent_transcript=path)
        self.assertEqual(report['status'], 'partial')
        self.assertEqual(report['agent_transcript']['reason'], 'not_unlinked_regular_file')
        self.assertIsNone(report['agent_transcript']['sha256'])

    def test_agent_source_size_record_and_line_limits_remain_visible(self):
        record = {'kind': 'windows_java_cleanup', 'success': False}
        path = self.agent_source([record] * 129)
        raw = path.read_bytes()
        report = collector.collect(self.root, agent_transcript=path)
        source = report['agent_transcript']
        self.assertEqual(report['status'], 'partial')
        self.assertEqual(source['status'], 'truncated')
        self.assertEqual(source['truncation_reasons'], ['transcript_records_limit'])
        self.assertEqual(len(source['evidence']['records']), 128)
        self.assertEqual(source['bytes'], len(raw))
        self.assertEqual(source['sha256'], hashlib.sha256(raw).hexdigest())
        self.assertTrue(source['truncated'])
        path.write_text(json.dumps(record) + '\n' + 'SECRET_LONG_LINE' * 5000)
        report = collector.collect(self.root, agent_transcript=path)
        self.assertEqual(report['status'], 'partial')
        self.assertEqual(report['agent_transcript']['truncation_reasons'], ['transcript_line_characters_limit'])
        self.assertEqual(report['agent_transcript']['evidence']['records'], [record])
        self.assertNotIn('SECRET_', json.dumps(report))
        with mock.patch.object(collector, 'read_checked', side_effect=AssertionError('must not read')):
            report = collector.collect(self.root, self.limits(source_file_bytes=8), agent_transcript=path)
        self.assertEqual(report['status'], 'partial')
        self.assertEqual(report['agent_transcript']['status'], 'skipped')
        self.assertEqual(report['agent_transcript']['reason'], 'source_file_bytes_limit')
        self.assertIsNone(report['agent_transcript']['sha256'])

    def test_agent_cli_collects_failures_and_errors_without_claiming_acceptance(self):
        records = self.agent_fixture()
        records.insert(1, {**self.agent_diagnostics_fixture(), 'result': 'timeout',
                           'message': 'SECRET_TIMEOUT_DIAGNOSTIC'})
        records[2:2] = self.agent_lifecycle_fixtures()
        records.insert(4, self.agent_production_fixture())
        records[-1].update(success=False, primary_failed=True, failure_stage='correction',
                           error='SECRET_FAILURE_ERROR')
        path = self.agent_source(records)
        java = self.private_source('private-java.txt', '{"kind":"pass","windows_full_acceptance":false}\n')
        output = self.base / 'report.json'
        command = [sys.executable, str(Path(collector.__file__)), '--root', str(self.root),
                   '--output', str(output), '--agent-transcript', str(path), '--java-transcript', str(java)]
        collected = subprocess.run(command, capture_output=True, text=True, timeout=10)
        self.assertEqual(collected.returncode, 0, collected.stderr)
        report = json.loads(output.read_text())
        self.assertFalse(report['agent_transcript']['evidence']['records'][-1]['success'])
        self.assertEqual(report['agent_transcript']['evidence']['records'][1]['result'], 'timeout')
        self.assertEqual(report['agent_transcript']['evidence']['records'][2:4], self.agent_lifecycle_fixtures())
        self.assertEqual(report['agent_transcript']['evidence']['records'][4], self.agent_production_fixture())
        self.assertFalse(report['java_transcript']['evidence']['records'][0]['windows_full_acceptance'])
        self.assertEqual(report['acceptance_result'], 'not_evaluated')
        self.assertEqual(json.loads(collected.stdout)['acceptance_result'], 'not_evaluated')
        self.assertNotIn('SECRET_', collected.stdout + collected.stderr + output.read_text())
        path.unlink()
        failed = subprocess.run(command, capture_output=True, text=True, timeout=10)
        self.assertEqual(failed.returncode, 1)
        self.assertEqual(json.loads(output.read_text())['agent_transcript']['status'], 'error')
        self.assertNotIn(str(self.root), failed.stdout + failed.stderr + output.read_text())

    def test_oversized_log_is_skipped_without_reading_or_hashing(self):
        self.log('SECRET_OVERSIZED_CONTENT')
        with mock.patch.object(collector, 'read_checked', side_effect=AssertionError('must not read')):
            report = collector.collect(self.root, self.limits(file_bytes=8))
        self.assertEqual(report['status'], 'partial')
        self.assertEqual(report['files'][0]['status'], 'skipped')
        self.assertEqual(report['files'][0]['reason'], 'file_bytes_limit')
        self.assertIsNone(report['files'][0]['sha256'])

    def test_maximum_file_size_is_accepted(self):
        path = self.log()
        report = collector.collect(self.root, self.limits(file_bytes=path.stat().st_size))
        self.assertEqual(report['status'], 'complete')

    def test_file_limit_is_reported(self):
        for number in range(4):
            self.log(name=f'hs_err_pid{number}.log')
        report = collector.collect(self.root, self.limits(files=2))
        self.assertEqual(report['status'], 'partial')
        self.assertEqual(len(report['files']), 2)
        self.assertEqual(report['issues'][0]['reason'], 'file_count_limit')

    def test_entry_and_depth_limits_are_reported(self):
        self.log(name='one/two/hs_err_pid1.log')
        report = collector.collect(self.root, self.limits(depth=0))
        self.assertEqual(report['status'], 'partial')
        self.assertEqual(report['issues'][0]['reason'], 'depth_limit')
        report = collector.collect(self.root, self.limits(directory_entries=1))
        self.assertEqual(report['status'], 'partial')
        self.assertEqual(report['directory_entries_scanned'], 1)
        self.assertEqual(report['issues'][0]['reason'], 'directory_entries_limit')

    def test_frame_and_line_limits_preserve_truncation(self):
        self.log()
        report = collector.collect(self.root, self.limits(frames_per_section=1))
        self.assertEqual(report['status'], 'partial')
        item = report['files'][0]
        self.assertEqual(item['status'], 'truncated')
        self.assertEqual(len(item['evidence']['native_frames']), 1)
        self.assertEqual(len(item['evidence']['java_frames']), 1)
        self.log(HOTSPOT + '\n' + ('SECRET_LONG_LINE' * 400))
        report = collector.collect(self.root)
        self.assertEqual(report['files'][0]['status'], 'truncated')
        self.assertIn('line_characters_limit', report['files'][0]['reasons'])
        self.assertNotIn('SECRET_', json.dumps(report))

    def test_repeated_problematic_frames_respect_the_frame_limit(self):
        self.log(HOTSPOT.replace('# No core dump', '# Problematic frame:\n# C  [jvm.dll+0x999]\n# No core dump'))
        report = collector.collect(self.root, self.limits(frames_per_section=1))
        item = report['files'][0]
        self.assertEqual(item['status'], 'truncated')
        self.assertEqual(len(item['evidence']['problematic_frame']), 1)
        self.assertIn('problematic_frame_limit', item['reasons'])

    def test_file_symlink_is_not_followed(self):
        outside = self.base / 'outside.log'
        outside.write_text('SECRET_OUTSIDE_FILE')
        self.link(outside, self.root / 'hs_err_pid99.log')
        with mock.patch.object(collector, 'read_checked', side_effect=AssertionError('must not read')):
            report = collector.collect(self.root)
        self.assertEqual(report['status'], 'partial')
        self.assertEqual(report['files'], [])
        self.assertEqual(report['issues'][0]['reason'], 'symlink_or_reparse_point')
        self.assertNotIn('SECRET_', json.dumps(report))

    def test_directory_symlink_cannot_escape_scan_root(self):
        outside = self.base / 'outside'
        outside.mkdir()
        (outside / 'hs_err_pid99.log').write_text('SECRET_OUTSIDE_DIRECTORY')
        self.link(outside, self.root / 'linked-directory', directory=True)
        report = collector.collect(self.root)
        self.assertEqual(report['files'], [])
        self.assertEqual(report['issues'][0]['reason'], 'symlink_or_reparse_point')

    def assert_gc_namespace_private(self, report):
        rendered = json.dumps(report).lower()
        for private in ('cedar-gc', '987654321', 'secret_gc'):
            self.assertNotIn(private, rendered)
        self.assertEqual([item['relative_filename'] for item in report['files']],
                         ['distribution/hs_err_pid314.log'])
        self.assertEqual(report['files'][0]['status'], 'collected')

    def test_enclosing_crash_scan_excludes_gc_files_and_directory_contents(self):
        self.log(name='distribution/hs_err_pid314.log')
        distribution = self.root / 'distribution'
        for name in ('cedar-gc-987654321.log', 'CEDAR-GC-987654321.log.0',
                     'CeDaR-gC-987654321.log.1'):
            (distribution / name).write_text('SECRET_GC_RAW_LOG')
        private_directory = distribution / 'CeDaR-Gc-987654321-directory'
        private_directory.mkdir()
        (private_directory / 'hs_err_pid987654321.log').write_text('SECRET_GC_NESTED')
        real_scandir = collector.os.scandir

        def guarded_scandir(path):
            self.assertNotEqual(Path(path), private_directory)
            return real_scandir(path)

        with mock.patch.object(collector.os, 'scandir', side_effect=guarded_scandir):
            report = collector.collect(self.root)
        self.assertEqual(report['status'], 'complete')
        self.assertEqual(report['issues'], [])
        self.assert_gc_namespace_private(report)

    def test_enclosing_crash_scan_excludes_gc_dangling_file_and_directory_links(self):
        self.log(name='distribution/hs_err_pid314.log')
        distribution = self.root / 'distribution'
        outside_file = self.base / 'private-file'
        outside_file.write_text('SECRET_GC_LINKED_LOG')
        outside_directory = self.base / 'private-directory'
        outside_directory.mkdir()
        (outside_directory / 'hs_err_pid987654321.log').write_text('SECRET_GC_LINKED_DIRECTORY')
        self.link(self.base / 'missing-target', distribution / 'cedar-gc-987654321.log')
        self.link(outside_file, distribution / 'CEDAR-GC-987654321.log.0')
        self.link(outside_directory, distribution / 'CeDaR-Gc-987654321-directory', directory=True)
        report = collector.collect(self.root)
        self.assertEqual(report['status'], 'complete')
        self.assertEqual(report['issues'], [])
        self.assert_gc_namespace_private(report)

    def test_enclosing_crash_scan_skips_gc_namespace_before_windows_reparse_metadata(self):
        self.log(name='distribution/hs_err_pid314.log')
        distribution = self.root / 'distribution'
        private_file = distribution / 'CeDaR-Gc-987654321.log'
        private_file.write_text('SECRET_GC_REPARSE_FILE')
        private_directory = distribution / 'CEDAR-GC-987654321-directory'
        private_directory.mkdir()
        real_lstat = Path.lstat
        private_metadata_calls = []

        def reparse_lstat(path, *args, **kwargs):
            if path in (private_file, private_directory):
                private_metadata_calls.append(path)
                mode = stat.S_IFDIR if path == private_directory else stat.S_IFREG
                return SimpleNamespace(st_mode=mode | 0o600, st_file_attributes=0x400)
            return real_lstat(path, *args, **kwargs)

        with mock.patch.object(Path, 'lstat', autospec=True, side_effect=reparse_lstat):
            report = collector.collect(self.root)
        self.assertEqual(private_metadata_calls, [])
        self.assertEqual(report['status'], 'complete')
        self.assertEqual(report['issues'], [])
        self.assert_gc_namespace_private(report)

    def test_enclosing_crash_scan_skips_gc_stat_errors_without_hiding_other_errors(self):
        self.log(name='distribution/hs_err_pid314.log')
        distribution = self.root / 'distribution'
        private = distribution / 'CEDAR-GC-987654321.log.1'
        private.write_text('SECRET_GC_STAT_ERROR')
        ordinary = distribution / 'ordinary-unreadable'
        ordinary.write_text('unavailable')
        real_lstat = Path.lstat
        private_metadata_calls = []

        def failed_lstat(path, *args, **kwargs):
            if path == private:
                private_metadata_calls.append(path)
                raise PermissionError(13, 'SECRET_GC_STAT_ERROR', str(path))
            if path == ordinary:
                raise PermissionError(13, 'ordinary stat error', str(path))
            return real_lstat(path, *args, **kwargs)

        with mock.patch.object(Path, 'lstat', autospec=True, side_effect=failed_lstat):
            report = collector.collect(self.root)
        self.assertEqual(private_metadata_calls, [])
        self.assertEqual(report['status'], 'error')
        self.assertEqual(len(report['issues']), 1)
        self.assertEqual(report['issues'][0]['reason'], 'stat_failed')
        self.assertEqual(report['issues'][0]['relative_filename'], 'distribution/ordinary-unreadable')
        self.assert_gc_namespace_private(report)

    def test_root_and_root_ancestor_symlinks_are_rejected(self):
        linked = self.base / 'linked-root'
        self.link(self.root, linked, directory=True)
        (self.root / 'child').mkdir()
        for root in (linked, linked / 'child'):
            with self.subTest(root=root):
                report = collector.collect(root)
                self.assertEqual(report['status'], 'error')
                self.assertEqual(report['issues'][0]['reason'], 'unsafe_or_unavailable_root')

    def test_windows_junction_reparse_attribute_is_rejected(self):
        info = SimpleNamespace(st_mode=stat.S_IFDIR | 0o755, st_file_attributes=0x400)
        self.assertTrue(collector.is_link(info))
        self.assertFalse(collector.is_link(SimpleNamespace(st_mode=stat.S_IFDIR | 0o755)))
        self.log()
        original = collector.is_link
        with mock.patch.object(collector, 'is_link', side_effect=lambda value: (
                True if stat.S_ISREG(value.st_mode) else original(value))):
            report = collector.collect(self.root)
        self.assertEqual(report['status'], 'partial')
        self.assertEqual(report['files'], [])
        self.assertEqual(report['issues'][0]['reason'], 'symlink_or_reparse_point')

    def test_cached_windows_direntry_zero_identity_is_never_trusted(self):
        self.log()
        real_scandir = collector.os.scandir
        cached_stats = []

        @contextmanager
        def cached_scandir(directory):
            with real_scandir(directory) as entries:
                proxies = []
                for entry in entries:
                    cached = SimpleNamespace(st_mode=stat.S_IFREG | 0o600, st_size=0,
                                             st_ino=0, st_dev=0, st_nlink=0)
                    stat_method = mock.Mock(return_value=cached)
                    cached_stats.append(stat_method)
                    proxies.append(SimpleNamespace(name=entry.name, stat=stat_method))
                yield iter(proxies)

        with mock.patch.object(collector.os, 'scandir', side_effect=cached_scandir):
            report = collector.collect(self.root)
        self.assertEqual(report['status'], 'complete')
        self.assertEqual(report['files'][0]['status'], 'collected')
        self.assertTrue(cached_stats)
        for cached in cached_stats:
            cached.assert_not_called()

    def test_windows_short_alias_normalization_follows_no_link_checks(self):
        root = PureWindowsPath(r'C:\Users\RUNNER~1\Temp\cedar')
        path = root / 'hs_err_pid1.log'
        long_root = r'C:\Users\Runner Admin\Temp\cedar'
        long_path = long_root + r'\hs_err_pid1.log'
        info = SimpleNamespace(st_dev=8, st_ino=42)
        events = []

        def checked(value):
            events.append('check')
            return info

        def canonical(value, *, strict):
            self.assertTrue(strict)
            events.append('canonical')
            return long_root if value == root else long_path

        with mock.patch.object(collector, 'check_components', side_effect=checked), \
                mock.patch.object(collector.os.path, 'realpath', side_effect=canonical):
            result = collector.checked_windows_canonical_path(root, path, info)
        self.assertEqual(result, collector.windows_path_key('\\\\?\\' + long_path))
        self.assertEqual(events, ['check', 'check', 'canonical', 'canonical', 'check', 'check'])
        self.assertEqual(collector.windows_path_key(r'\\?\UNC\SERVER\share\file'),
                         collector.windows_path_key(r'\\server\share\file'))

    def test_windows_canonicalization_rejects_links_root_changes_and_escape(self):
        root = PureWindowsPath(r'C:\owned')
        path = root / 'hs_err_pid1.log'
        info = SimpleNamespace(st_dev=8, st_ino=42)
        changed = SimpleNamespace(st_dev=8, st_ino=43)
        with mock.patch.object(collector, 'check_components', side_effect=ValueError('symlink_or_reparse_point')), \
                mock.patch.object(collector.os.path, 'realpath') as canonical:
            with self.assertRaisesRegex(ValueError, 'symlink_or_reparse_point'):
                collector.checked_windows_canonical_path(root, path, info)
            canonical.assert_not_called()
        with mock.patch.object(collector, 'check_components', side_effect=[info, info, info, changed]), \
                mock.patch.object(collector.os.path, 'realpath', side_effect=[str(root), str(path)]):
            with self.assertRaisesRegex(ValueError, 'root_changed'):
                collector.checked_windows_canonical_path(root, path, info)
        with mock.patch.object(collector, 'check_components', return_value=info), \
                mock.patch.object(collector.os.path, 'realpath', side_effect=[str(root), r'C:\outside\hs_err_pid1.log']):
            with self.assertRaisesRegex(ValueError, 'canonical_path_outside_root'):
                collector.checked_windows_canonical_path(root, path, info)

    def test_windows_read_keeps_handle_path_root_and_file_identity_checks(self):
        path = self.log()
        expected, root_info = path.lstat(), self.root.lstat()
        canonical = collector.windows_path_key(r'C:\Users\Runner Admin\owned\hs_err_pid314.log')
        # Exercise the Windows branch with real file descriptors on every host;
        # only the native handle-path API and canonical spellings are replaced.
        with mock.patch.object(collector.os, 'name', 'nt'), \
                mock.patch.object(collector.os, 'supports_dir_fd', set()), \
                mock.patch.object(collector, 'checked_windows_canonical_path', return_value=canonical), \
                mock.patch.object(collector, 'windows_handle_path', return_value=canonical) as handle_path:
            actual = collector.read_checked(self.root, path, expected, root_info, collector.LIMITS['file_bytes'])
            self.assertEqual(actual, HOTSPOT.encode())
            handle_path.return_value = collector.windows_path_key(r'C:\outside\hs_err_pid314.log')
            with self.assertRaisesRegex(ValueError, 'opened_path_changed'):
                collector.read_checked(self.root, path, expected, root_info, collector.LIMITS['file_bytes'])
            handle_path.return_value = canonical
            stale = SimpleNamespace(st_dev=expected.st_dev, st_ino=expected.st_ino + 1, st_size=expected.st_size)
            with self.assertRaisesRegex(ValueError, 'file_changed'):
                collector.read_checked(self.root, path, stale, root_info, collector.LIMITS['file_bytes'])
            with mock.patch.object(collector, 'check_root_identity', side_effect=ValueError('root_changed')), \
                    mock.patch.object(collector.os, 'fdopen') as reader:
                with self.assertRaisesRegex(ValueError, 'root_changed'):
                    collector.read_checked(self.root, path, expected, root_info, collector.LIMITS['file_bytes'])
                reader.assert_not_called()

    @unittest.skipUnless(os.name == 'nt', 'Native Windows identity and short-path integration check')
    def test_windows_native_identity_and_short_path_metadata(self):
        import ctypes
        from ctypes import wintypes
        path = self.log()
        metadata = {'kind': 'windows_collector_filesystem', 'platform': 'windows',
                    'python_version': list(sys.version_info[:3])}
        failure = False
        try:
            with os.scandir(self.root) as entries:
                cached = next(entry for entry in entries if entry.name == path.name).stat(follow_symlinks=False)
            fresh = path.lstat()
            fd = os.open(path, os.O_RDONLY | os.O_BINARY)
            try:
                opened = os.fstat(fd)
                metadata.update({
                    'direntry_inode_zero': cached.st_ino == 0, 'direntry_device_zero': cached.st_dev == 0,
                    'direntry_nlink': cached.st_nlink, 'lstat_nlink': fresh.st_nlink,
                    'fstat_nlink': opened.st_nlink, 'lstat_inode_nonzero': fresh.st_ino != 0,
                    'fstat_inode_nonzero': opened.st_ino != 0,
                    'lstat_fstat_identity_equal': (fresh.st_dev, fresh.st_ino) == (opened.st_dev, opened.st_ino),
                    'canonical_matches_handle': collector.checked_windows_canonical_path(
                        self.root, path, self.root.lstat()) == collector.windows_handle_path(fd),
                })
            finally:
                os.close(fd)
            function = ctypes.WinDLL('kernel32', use_last_error=True).GetShortPathNameW
            function.argtypes = (wintypes.LPCWSTR, wintypes.LPWSTR, wintypes.DWORD)
            function.restype = wintypes.DWORD
            buffer = ctypes.create_unicode_buffer(32768)
            length = function(str(self.root), buffer, len(buffer))
            available = 0 < length < len(buffer)
            metadata['short_path_available'] = available
            if available:
                short_root = Path(buffer.value)
                metadata['short_spelling_differs'] = collector.windows_path_key(short_root) != collector.windows_path_key(
                    os.path.realpath(self.root, strict=True))
                metadata['short_canonical_equivalent'] = collector.windows_path_key(
                    os.path.realpath(short_root, strict=True)) == collector.windows_path_key(
                    os.path.realpath(self.root, strict=True))
                result = collector.collect(short_root)
                metadata['short_path_collected'] = result['status'] == 'complete' and len(result['files']) == 1
            else:
                metadata['short_path_error_code'] = ctypes.get_last_error()
        except (OSError, ValueError) as error:
            failure = True
            metadata['error_type'] = type(error).__name__
            if getattr(error, 'winerror', None) is not None:
                metadata['error_code'] = error.winerror
        # Numeric versions, counts and booleans only: never runner paths,
        # inherited environment values or a raw exception message.
        print(json.dumps(metadata, sort_keys=True), flush=True)
        self.assertFalse(failure, 'Native Windows metadata check failed; see sanitized metadata')
        self.assertTrue(metadata['lstat_fstat_identity_equal'])
        self.assertTrue(metadata['canonical_matches_handle'])
        self.assertEqual(metadata['lstat_nlink'], 1)
        self.assertEqual(metadata['fstat_nlink'], 1)
        if metadata['short_path_available']:
            self.assertTrue(metadata['short_canonical_equivalent'])
            self.assertTrue(metadata['short_path_collected'])

    @unittest.skipUnless(os.name == 'nt', 'Windows junction integration check')
    def test_real_windows_junction_cannot_escape_scan_root(self):
        outside = self.base / 'outside-junction-target'
        outside.mkdir()
        (outside / 'hs_err_pid99.log').write_text('SECRET_JUNCTION_TARGET')
        junction = self.root / 'junction'
        result = subprocess.run(['cmd', '/d', '/c', 'mklink', '/J', str(junction), str(outside)],
                                capture_output=True, timeout=10)
        if result.returncode:
            self.skipTest('Runner does not permit creating a synthetic junction')
        try:
            report = collector.collect(self.root)
            self.assertEqual(report['status'], 'partial')
            self.assertEqual(report['files'], [])
            self.assertEqual(report['issues'][0]['reason'], 'symlink_or_reparse_point')
        finally:
            os.rmdir(junction)

    def test_hard_link_is_skipped(self):
        source = self.base / 'synthetic-hardlink-target.log'
        source.write_text('SECRET_HARDLINK_TARGET')
        try:
            os.link(source, self.root / 'hs_err_pid99.log')
        except OSError as error:
            self.skipTest('Runner does not permit hardlinks: ' + type(error).__name__)
        report = collector.collect(self.root)
        self.assertEqual(report['files'][0]['reason'], 'hard_link')
        self.assertEqual(report['status'], 'partial')

    @unittest.skipUnless(hasattr(os, 'mkfifo'), 'POSIX special-file check')
    def test_special_file_is_skipped_without_blocking(self):
        os.mkfifo(self.root / 'hs_err_pid99.log')
        report = collector.collect(self.root)
        self.assertEqual(report['files'][0]['reason'], 'not_regular_file')

    def test_missing_root_and_io_failures_remain_visible(self):
        report = collector.collect(self.root / 'missing')
        self.assertEqual(report['status'], 'error')
        self.log()
        with mock.patch.object(collector, 'read_checked', side_effect=PermissionError(13, 'SECRET_ERROR_CONTENT')):
            report = collector.collect(self.root)
        self.assertEqual(report['status'], 'error')
        self.assertEqual(report['files'][0]['errno'], 13)
        self.assertNotIn('SECRET_', json.dumps(report))
        with mock.patch.object(collector.os, 'scandir', side_effect=PermissionError(13, 'SECRET_SCAN_ERROR')):
            report = collector.collect(self.root)
        self.assertEqual(report['issues'][0]['reason'], 'directory_scan_failed')
        self.assertEqual(report['status'], 'error')

    def test_file_changed_after_stat_is_not_collected(self):
        path = self.log()
        expected = path.stat()
        path.write_text('SECRET_REPLACEMENT')
        with self.assertRaisesRegex(ValueError, 'file_changed'):
            collector.read_checked(self.root, path, expected, self.root.stat(), collector.LIMITS['file_bytes'])

    def test_linux_signal_and_internal_error_headers(self):
        self.log(HOTSPOT.replace('EXCEPTION_ACCESS_VIOLATION (0xc0000005)', 'SIGSEGV (0xb)'))
        report = collector.collect(self.root)
        self.assertEqual(report['files'][0]['evidence']['exception']['name'], 'SIGSEGV')
        self.log(HOTSPOT.replace(
            'EXCEPTION_ACCESS_VIOLATION (0xc0000005) at pc=0x00007fff01234567, pid=314, tid=159',
            'Internal Error (SECRET_SOURCE_PATH.cpp:27), pid=314, tid=159'))
        report = collector.collect(self.root)
        self.assertEqual(report['files'][0]['evidence']['exception']['name'], 'Internal Error')
        self.assertNotIn('SECRET_', json.dumps(report))

    def test_cli_writes_json_and_reports_collection_failure(self):
        output = self.base / 'report.json'
        command = [sys.executable, str(Path(collector.__file__)), '--root', str(self.root), '--output', str(output)]
        empty = subprocess.run(command, capture_output=True, text=True, timeout=10)
        self.assertEqual(empty.returncode, 0, empty.stderr)
        self.assertEqual(json.loads(output.read_text())['files'], [])
        self.assertNotIn('agent_transcript', json.loads(output.read_text()))
        self.log('SECRET_MALFORMED')
        failed = subprocess.run(command, capture_output=True, text=True, timeout=10)
        self.assertEqual(failed.returncode, 1)
        self.assertEqual(json.loads(output.read_text())['status'], 'error')
        self.assertNotIn('SECRET_', failed.stdout + failed.stderr + output.read_text())

    def test_cli_output_failure_is_nonzero_and_does_not_expose_paths(self):
        result = subprocess.run([sys.executable, str(Path(collector.__file__)), '--root', str(self.root),
                                 '--output', str(self.base / 'missing-parent' / 'report.json')],
                                capture_output=True, text=True, timeout=10)
        self.assertEqual(result.returncode, 1)
        self.assertEqual(json.loads(result.stderr)['reason'], 'report_write_failed')
        self.assertNotIn(str(self.base), result.stderr)

    def test_output_cannot_overwrite_a_raw_log_or_follow_a_link(self):
        raw = self.log()
        with self.assertRaisesRegex(ValueError, 'output_must_not_be_a_crash_log'):
            collector.write_report(raw, {})
        output = self.base / 'report.json'
        self.link(raw, output)
        with self.assertRaisesRegex(ValueError, 'unsafe_output'):
            collector.write_report(output, {})
        self.assertEqual(raw.read_text(), HOTSPOT)


if __name__ == '__main__':
    unittest.main()
