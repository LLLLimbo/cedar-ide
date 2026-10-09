#!/usr/bin/env python3
"""Collect bounded, sanitized HotSpot diagnostics from a generated scratch tree.

Raw hs_err logs, probe output, Java transcripts and minidumps must never be
uploaded as a consequence of this script. This report is diagnostic evidence,
not a Java acceptance test result. Only bounded fixture status fields, exception
fields, fixed runtime headers, and parsed frame fields survive.
Exit 0 means collection was complete (including no logs); exit 1 means that
collection was partial or failed. Neither exit status establishes acceptance.
"""
import argparse
import hashlib
import json
import ntpath
import os
from pathlib import Path
import re
import stat
import sys
import tempfile


LIMITS = {
    'files': 16,
    'file_bytes': 2 * 1024 * 1024,
    'directory_entries': 4096,
    'depth': 8,
    'frames_per_section': 64,
    'line_characters': 4096,
    'source_file_bytes': 8 * 1024 * 1024,
    'probe_cases': 6,
    'probe_stream_bytes': 64 * 1024,
    'transcript_records': 128,
    'transcript_line_characters': 64 * 1024,
}
LOG_NAME = re.compile(r'hs_err_pid[^/\\]*\.log\Z', re.IGNORECASE)
# GC control logs and every rotation/directory variant stay wholly private.
# Their dedicated collector exports numeric evidence without PID-bearing names.
GC_PRIVATE_PREFIX = 'cedar-gc'
FATAL_HEADER = 'A fatal error has been detected by the Java Runtime Environment:'
EXCEPTION = re.compile(
    r'(?P<name>EXCEPTION_[A-Z_]+|SIG[A-Z0-9]+)'
    r'(?:\s+\((?P<code>0x[0-9a-fA-F]+)\))?'
    r'(?:\s+at pc=(?P<pc>0x[0-9a-fA-F]+),\s*pid=(?P<pid>\d+),\s*tid=(?P<tid>\d+))?\Z')
VERSION = re.compile(r'(?<![A-Za-z0-9])(?:1\.)?\d{1,3}(?:[._]\d+)*(?:\+\d+)?(?:-[A-Za-z0-9._+]+)?(?![A-Za-z0-9])')
JAVA_TYPE = r'\[*(?:[BCDFIJSZ]|L[A-Za-z0-9_$/.]+;)'
JAVA_METHOD = re.compile(
    r'(?P<method>[A-Za-z_$][A-Za-z0-9_$./]*\.(?:[A-Za-z_$][A-Za-z0-9_$]*|<(?:init|clinit)>))'
    r'\((?P<parameters>(?:' + JAVA_TYPE + r')*)\)(?P<returns>V|' + JAVA_TYPE + r')(?=$|[+ \t])')
LIBRARY_FRAME = re.compile(
    r'\[(?P<library>[^\[\]\r\n]+?)\+(?P<offset>0x[0-9a-fA-F]+)\]'
    r'(?=$|\s)')
NATIVE_SYMBOL = re.compile(r'(?P<symbol>[A-Za-z_~?][A-Za-z0-9_.$:<>?@~]*)(?:\+(?:0x[0-9a-fA-F]+|\d+))?\Z')
LIBRARY_NAME = re.compile(r'[A-Za-z0-9_.-]+\.(?:dll|exe|dylib|so(?:\.\d+)*)\Z', re.IGNORECASE)
PROBE_CASES = (
    'version_ordinary_ascii_cwd', 'version_canonical_ascii_cwd',
    'version_ordinary_unicode_cwd', 'version_canonical_unicode_cwd',
    'hello_ordinary_unicode_cwd', 'hello_canonical_unicode_cwd',
)
PROBE_OUTCOMES = ('setup_error', 'spawn_error', 'execution_deadline_exceeded',
                  'supervision_error', 'child_nonzero_exit', 'child_exited')
SESSIONS = ('initial', 'restart_fresh_data', 'restart_same_data')
PHASES = ('after_all_clients_dropped', 'after_semantic_queries_and_resolve',
          'after_unsaved_correction', 'after_shutdown_or_failure_drop', 'after_jdk_symbol_query')
CHECKS = ('jdk_workspace_symbol', 'initialize', 'didOpen', 'semantic_diagnostics', 'hover', 'completion',
          'definition', 'didChange', 'diagnostic_error_cleared',
          'correction_specific_warning', 'source_bytes_unchanged', 'didClose')
TRANSCRIPT_FIELDS = {
    'pass': dict.fromkeys(('fresh_data_restart', 'same_data_restart', 'source_bytes_unchanged',
                          'fixture_removed', 'lazy_import_resolve_checked',
                          'frontend_or_agent_included', 'windows_full_acceptance'), 'bool')
            | {'elapsed_ms': 'count', 'sessions': 'count'},
    'session_semantics_pass': {'session': SESSIONS, 'elapsed_ms': 'count',
                               'initial_diagnostics': 'count', 'corrected_diagnostics': 'count',
                               'lazy_import_resolve_checked': 'bool', 'jdk_index_symbol_checked': 'bool', 'server_commands_executed': 'bool',
                               'checks': ('enum_list', CHECKS)},
    'session_cleanup': dict.fromkeys(('semantics_succeeded', 'shutdown_api_succeeded', 'client_dropped',
                                     'windows_root_handle_signaled', 'source_unchanged',
                                     'independent_job_zero_observation', 'listener_observation'), 'bool')
                       | {'session': SESSIONS, 'pid': 'u32', 'gracefully_exited': '?bool',
                          'root_exit_code': '?u32', 'shutdown_elapsed_ms': 'count',
                          'shutdown_terminal_reason': ('grace_expired', 'stdout_eof', 'root_exit',
                                                       'worker_stopped', 'closed_other', 'protocol_error',
                                                       'io_error', 'request_timeout', 'other_error',
                                                       'no_terminal_event', 'event_limit')},
    'source_bytes': {'phase': PHASES, 'unchanged': 'bool'},
    'fixture_cleanup': dict.fromkeys(('removed', 'source_unchanged_before_removal', 'sessions_succeeded'), 'bool'),
    'process_identity': {'session': SESSIONS, 'pid': 'u32', 'creation_time_100ns_since_1601': '?u64',
                         'retained_windows_observation_handle': 'bool'},
    'data_directory_witness': {'session': SESSIONS, 'metadata_in_expected_directory': 'bool'},
}
AGENT_LIFECYCLE_FAILURE_STAGES = (
    'none', 'setup', 'initialize', 'open', 'diagnostics', 'hover', 'task_start', 'task_identity',
    'language_stop', 'task_survival', 'task_cancel', 'java_survival', 'owner_death', 'agent_exit',
    'java_exit', 'task_exit', 'source', 'fixture_cleanup',
)
CORRECTION_RECOVERY_RESULTS = (
    'not_attempted', 'not_eligible', 'insufficient_budget', 'request_error',
    'acknowledgement_mismatch', 'timeout', 'malformed_events', 'truncated',
    'lagged', 'closed', 'matched',
)
AGENT_TRANSCRIPT_FIELDS = {
    'windows_java_session': dict.fromkeys((
        'semantic_checks_passed', 'exact_diagnostics', 'exact_definition', 'real_completion',
        'deferred_import_resolve', 'primary_identity_unchanged', 'two_atomic_edits',
        'advisory_command_skipped', 'actual_undo', 'actual_redo', 'versions_2_3_4_synced',
        'correction_change_acknowledged', 'correction_diagnostics', 'workflow_success',
        'correction_recovery_acknowledged', 'correction_recovery_witness',
        'correction_recovery_unversioned', 'correction_recovery_budget_sufficient',
        'source_unchanged', 'root_observed_live',
        'root_identity_verified', 'jdk_symbol_verified', 'shutdown_api_succeeded',
        'root_handle_signaled', 'gracefully_exited'), 'bool')
        | {'session': ('integer_range', 1, 3), 'mode': ('initial', 'fresh_data', 'reused_data'),
           'initialization_ms': 'count', 'root_exit_code': '?u32', 'shutdown_elapsed_ms': 'count',
           'correction_recovery_result': CORRECTION_RECOVERY_RESULTS,
           'correction_recovery_attempts': ('integer_range', 0, 1),
           'correction_change_result': ('not_attempted', 'request_error', 'acknowledgement_mismatch',
                                         'acknowledged')},
    'windows_java_diagnostics': dict.fromkeys((
        'polls', 'events', 'diagnostic_batches', 'uri_match_batches', 'parsed_batches',
        'version_match_batches', 'unversioned_batches', 'eligible_batches', 'eligible_empty_batches',
        'eligible_error_free_batches', 'error_diagnostics', 'warning_diagnostics',
        'expected_message_diagnostics', 'expected_severity_diagnostics', 'expected_range_diagnostics',
        'expected_joint_diagnostics', 'eligible_expected_joint_diagnostics',
        'eligible_error_diagnostics', 'matching_batches'), ('integer_range', 0, 65535))
        | {'session': ('integer_range', 1, 3), 'phase': ('initial', 'correction'),
           'result': ('matched', 'timeout', 'request_error', 'malformed_events', 'truncated', 'lagged', 'closed'),
           'counters_saturated': 'bool', 'elapsed_saturated': 'bool',
           'elapsed_ms': ('integer_range', 0, 300000)},
    'windows_java_correction_hover': {
        'session': ('integer_range', 1, 3),
        'result': ('matched', 'no_match', 'request_error'),
        'elapsed_ms': ('integer_range', 0, 300000), 'elapsed_saturated': 'bool',
    },
    'windows_java_correction_recovery': dict.fromkeys((
        'acknowledged', 'witness', 'unversioned', 'budget_sufficient',
        'cleanup_reserve_guaranteed', 'elapsed_saturated', 'counters_saturated'), 'bool')
        | {'session': ('integer_range', 1, 3), 'result': CORRECTION_RECOVERY_RESULTS,
           'original_result': ('matched', 'timeout', 'request_error', 'malformed_events',
                               'truncated', 'lagged', 'closed'),
           'attempts': ('integer_range', 0, 1),
           'available_budget_ms': ('integer_range', 0, 360000),
           'required_budget_ms': ('integer_range', 0, 165000),
           'request_timeout_ms': ('integer_range', 0, 75000),
           'witness_dispatch_window_ms': ('integer_range', 0, 15000),
           'event_poll_timeout_ms': ('integer_range', 0, 75000),
           'elapsed_ms': ('integer_range', 0, 300000),
           'polls': ('integer_range', 0, 65535), 'events': ('integer_range', 0, 65535)},
    'windows_java_cleanup': dict.fromkeys((
        'agent_exit_zero', 'source_unchanged', 'observed_roots_exited', 'synthetic_root_removed',
        'success', 'spontaneous_success', 'workflow_success', 'primary_failed', 'cleanup_failed'), 'bool')
        | {'sessions_completed': ('integer_range', 0, 3),
           'failure_stage': ('none', 'setup', 'initialize', 'open', 'diagnostics', 'hover',
                             'definition', 'completion', 'resolve', 'apply', 'undo', 'redo',
                             'sync', 'correction', 'close', 'stop', 'root_exit', 'agent_exit',
                             'fixture_cleanup')},
    'windows_java_concurrency': dict.fromkeys((
        'language_stop_preserved_task', 'task_cancel_preserved_java', 'hover_after_cancel',
        'task_identities_verified', 'task_locks_verified', 'tasks_exited', 'task_locks_released',
        'task_caps_not_reached', 'source_unchanged', 'primary_failed', 'cleanup_failed', 'success'), 'bool')
        | {'tasks_started': ('integer_range', 0, 2), 'tasks_completed': ('integer_range', 0, 2),
           'failure_stage': AGENT_LIFECYCLE_FAILURE_STAGES},
    'windows_java_forced_cleanup': dict.fromkeys((
        'java_observed_live', 'java_identity_verified', 'task_observed_live', 'task_identity_verified',
        'task_lock_verified', 'owner_death_injected', 'agent_exit_observed', 'agent_exit_nonzero',
        'java_exit_observed', 'task_exit_observed', 'task_lock_released', 'task_cap_not_reached',
        'source_unchanged', 'synthetic_root_removed', 'primary_failed', 'cleanup_failed', 'success',
        'elapsed_saturated'), 'bool')
        | {'java_exit_code': '?u32', 'task_exit_code': '?u32',
           'failure_stage': AGENT_LIFECYCLE_FAILURE_STAGES, 'elapsed_ms': ('integer_range', 0, 300000)},
    'windows_java_production': dict.fromkeys((
        'async_start_exercised', 'async_start_begin_acknowledged',
        'async_start_read_while_starting', 'async_start_ready',
        'java_capabilities', 'generic_start_rejected', 'untrusted_start_rejected', 'root_observed_live',
        'root_identity_verified', 'semantic_diagnostics', 'exact_definition', 'real_completion',
        'deferred_import_resolve', 'actual_editor_apply_undo_redo', 'versions_2_3_4_synced',
        'correction_acknowledged', 'correction_diagnostics', 'source_unchanged', 'stop_outcome_verified',
        'diagnostics_refresh_exercised', 'diagnostics_refresh_supported',
        'diagnostics_refresh_requested', 'diagnostics_refresh_witness',
        'diagnostics_refresh_unversioned',
        'shutdown_response_received', 'exit_frame_completed', 'cleanup_joined', 'root_handle_signaled',
        'client_reaped', 'synthetic_root_removed', 'primary_failed', 'cleanup_failed', 'success',
        'elapsed_saturated'), 'bool')
        | {'route': ('normal_agent_client',), 'stop_status': ('not_attempted', 'graceful', 'forced', 'error'),
           'stop_reason': ('not_attempted', 'root_exited', 'grace_expired', 'aborted',
                           'transport_failure', 'worker_panicked'),
           'root_exit_code': '?u32', 'elapsed_ms': ('integer_range', 0, 300000),
           'failure_stage': ('none', 'setup', 'initialize', 'open', 'diagnostics', 'hover',
                             'definition', 'completion', 'resolve', 'apply', 'undo', 'redo',
                             'sync', 'correction', 'close', 'stop', 'root_exit', 'agent_exit',
                             'fixture_cleanup')},
    # Quick-only workspace type witness. No query, type name, URI, source,
    # provider response, protocol or private diagnostic text is retained.
    'windows_java_workspace_types': dict.fromkeys((
        'exercised', 'capability_supported', 'provider_supported', 'target_unopened',
        'exact_type_name', 'exact_type_uri', 'exact_declaration_range', 'negative_query_empty',
        'resolved_path_exact', 'ordinary_read_exact', 'actual_frontend_navigation',
        'dirty_buffer_reused', 'undo_redo_preserved', 'source_unchanged',
        'root_handle_signaled', 'client_reaped', 'synthetic_root_removed',
        'primary_failed', 'cleanup_failed', 'success', 'elapsed_saturated'), 'bool')
        | {'failure_stage': ('none', 'setup', 'support', 'query', 'negative_query',
                             'resolve', 'read', 'frontend'),
           'elapsed_ms': ('integer_range', 0, 240000)},
    # Quick-only implementation witness: no source, symbol, URI, path, revision,
    # protocol data, inherited environment or arbitrary error is retained.
    'windows_java_implementations': dict.fromkeys((
        'exercised', 'capability_supported', 'provider_supported', 'query_version_acknowledged',
        'targets_unopened', 'exact_type_uris', 'exact_type_ranges', 'exact_method_uri',
        'exact_method_range', 'inherited_method_absent', 'negative_query_empty',
        'utf16_ranges_exact', 'resolved_path_exact', 'ordinary_read_exact',
        'actual_frontend_navigation', 'full_selection_preserved', 'dirty_buffer_reused',
        'undo_redo_preserved', 'retained_context_preserved', 'source_files_unchanged',
        'root_handle_signaled', 'client_reaped', 'synthetic_root_removed',
        'primary_failed', 'cleanup_failed', 'success', 'elapsed_saturated'), 'bool')
        | {'type_result_count': ('integer_range', 0, 128),
           'method_result_count': ('integer_range', 0, 128),
           'negative_result_count': ('integer_range', 0, 128),
           'failure_stage': ('none', 'setup', 'support', 'open', 'type_query', 'method_query',
                             'negative_query', 'resolve', 'read', 'frontend', 'close'),
           'elapsed_ms': ('integer_range', 0, 240000)},
    # Quick-only organize-imports receipt. In particular, neither source edits,
    # candidate names/URIs nor raw JDT responses are ever copied into evidence.
    'windows_java_organize_imports': dict.fromkeys((
        'exercised', 'supported', 'left_candidate_indexed', 'right_candidate_indexed',
        'unsaved_type_indexed', 'independent_type_indexed', 'unsaved_version_acknowledged',
        'sorted_retained_imports', 'unused_import_removed', 'unsaved_unique_import_added',
        'preview_unchanged', 'cancel_unchanged', 'actual_frontend_apply', 'one_undo_exact',
        'one_redo_exact', 'draft_versions_synced', 'ambiguous_candidates_skipped',
        'independent_import_added', 'source_files_unchanged', 'root_handle_signaled',
        'client_reaped', 'synthetic_root_removed', 'primary_failed', 'cleanup_failed',
        'success', 'elapsed_saturated'), 'bool')
        | {'main_edit_count': ('integer_range', 0, 1024),
           'ambiguity_edit_count': ('integer_range', 0, 1024),
           'observed_editor_stages': ('integer_range', 0, 5),
           'failure_stage': ('none', 'setup', 'support', 'index_witness', 'unsaved_sync',
                             'organize', 'preview', 'cancel', 'apply', 'undo', 'redo',
                             'ambiguity', 'close'),
           'elapsed_ms': ('integer_range', 0, 240000)},
}
# The diagnostic route shares bounded semantic witnesses with the shipping
# acceptance, but has its own kind and one exact route. Never expand the shipping
# route namespace or publish the private GC selection/PID/log paths here.
AGENT_TRANSCRIPT_FIELDS['windows_java_gc_control'] = {
    **AGENT_TRANSCRIPT_FIELDS['windows_java_production'],
    'route': ('diagnostic_agent_normal_client',),
}

# The required bounded idle workflow is separate from both Quick and the
# unchanged long resource baseline. Only fixed scalar witnesses survive.
IDLE_FIXED_BUDGETS = {
    'primary_deadline_ms': 360000, 'outer_deadline_ms': 480000,
    'cleanup_reserve_ms': 120000, 'request_timeout_ms': 75000,
    'spontaneous_dispatch_window_ms': 60000, 'diagnostic_wait_admission_ms': 135000,
    'initial_idle_ms': 30000, 'recovery_budget_ms': 165000,
    'recovery_admission_ms': 240000, 'close_budget_ms': 75000,
    'stop_budget_ms': 75000, 'root_exit_budget_ms': 3000,
    'client_reap_budget_ms': 30000, 'cleanup_bookkeeping_ms': 9000,
}
AGENT_TRANSCRIPT_FIELDS['windows_java_idle_correction'] = {
    **AGENT_TRANSCRIPT_FIELDS['windows_java_production'],
    **dict.fromkeys(('spontaneous_success', 'recovery_acknowledged', 'recovery_witness',
                    'recovery_unversioned', 'recovery_budget_sufficient', 'workflow_success',
                    'primary_deadline_met', 'cleanup_deadline_met', 'cleanup_reserve_preserved',
                    'deadline_failed'), 'bool'),
    **{key: ('integer_range', value, value) for key, value in IDLE_FIXED_BUDGETS.items()},
    'spontaneous_result': ('matched', 'timeout', 'request_error', 'malformed_events',
                           'truncated', 'lagged', 'closed'),
    'spontaneous_matching_batches': ('integer_range', 0, 65535),
    'recovery_attempts': ('integer_range', 0, 1),
    'recovery_result': CORRECTION_RECOVERY_RESULTS,
    'recovery_available_budget_ms': ('integer_range', 0, 360000),
    'primary_elapsed_ms': ('integer_range', 0, 480000),
    'cleanup_started_ms': ('integer_range', 0, 480000),
    'elapsed_ms': ('integer_range', 0, 480000),
}

# One fixed normal-route Maven pair. Nested schemas are declared here; no
# arbitrary object, path, diagnostic text or environment field is forwarded.
MAVEN_CASE_FIELDS = dict.fromkeys((
    'java_capabilities', 'generic_start_rejected', 'untrusted_start_rejected',
    'model_without_session_rejected', 'async_start_begin_acknowledged',
    'async_start_read_while_starting', 'async_start_ready', 'root_identity_verified',
    'root_observed_live', 'maven_nature', 'custom_source', 'compiler_17',
    'exact_dependency_reference', 'offline_pom_diagnostic',
    'owned_project_missing_library_diagnostic', 'hover', 'completion',
    'deliberate_type_diagnostic', 'dirty_change_acknowledged', 'no_autosave',
    'stale_startup_rejected', 'changed_pom_restart_required', 'source_unchanged',
    'pom_expected', 'repository_inputs_unchanged', 'dependency_jar_present_before',
    'dependency_jar_present_after', 'dependency_pom_present_before',
    'dependency_pom_present_after', 'cleanup_joined', 'shutdown_response_received',
    'exit_frame_completed', 'stop_outcome_verified', 'root_handle_signaled',
    'model_after_stop_rejected', 'client_reaped', 'synthetic_root_removed',
    'primary_failed', 'cleanup_failed', 'success'), 'bool') | {
        'case': ('present', 'missing'),
        'failure_stage': ('setup', 'trust', 'startup', 'model', 'semantics', 'pom_change',
                          'stop', 'root_exit', 'client_exit', 'fixture_cleanup', 'none'),
        'model_status': ('unavailable', 'imported', 'unresolved'),
        'model_queries': ('integer_range', 0, 240),
        'unexpected_dependency_references': ('integer_range', 0, 65535),
        'generated_metadata_files': ('integer_range', 0, 65535),
        'lifecycle_metadata_files': ('integer_range', 0, 6),
        'lifecycle_metadata_mask': ('integer_range', 0, 63),
        'generated_data_files': ('integer_range', 0, 4096),
        'generated_data_bytes': ('integer_range', 0, 134217728),
        'generated_project_files': ('integer_range', 0, 256),
        'generated_project_bytes': ('integer_range', 0, 16777216),
        'foreign_repository_files': ('integer_range', 0, 65535),
        'stop_status': ('nullable_enum', ('graceful', 'forced', 'error')),
        'stop_reason': ('nullable_enum', ('root_exited', 'grace_expired', 'aborted',
                                        'transport_failure', 'worker_panicked')),
        'root_exit_code': '?u32',
    }
MAVEN_PROBE_ERROR_CODES = ('none', 'unsupported_operation', 'run_disabled',
                         'language_not_running', 'language_maven_session_required',
                         'language_maven_unsupported', 'language_maven_restart_required',
                         'language_maven_invalid_model', 'transport_failure', 'other')
MAVEN_CASE_FIELDS.update({
    'model_probe_outcome': ('not_attempted', 'request_failed', 'non_language_payload',
                            'response_received', 'model_rejected', 'not_ready', 'ready',
                            'budget_exhausted'),
    'model_error_code': MAVEN_PROBE_ERROR_CODES,
    'model_rejection': ('none', 'profile_or_pom', 'status', 'missing_classpath',
                        'classpath_bound', 'missing_source_path', 'escaped_source_path',
                        'dependency_resolution', 'dependency_origin', 'entry_kind',
                        'foreign_or_duplicate_reference'),
    'event_probe_outcome': ('not_attempted', 'request_failed', 'non_language_payload',
                            'response_received', 'events_accepted', 'events_rejected'),
    'event_error_code': MAVEN_PROBE_ERROR_CODES,
    'event_rejection': ('none', 'truncated', 'missing_events', 'unknown_event',
                        'closed_event', 'lagged_event', 'missing_event_type',
                        'missing_diagnostic_uri', 'missing_diagnostics',
                        'unexpected_pom_diagnostic', 'unexpected_source_diagnostic',
                        'unexpected_project_diagnostic',
                        'foreign_document', 'uri_encoding'),
    'rejected_diagnostic_origin': ('none', 'pom', 'source', 'foreign', 'missing', 'owned_project_root'),
    'rejected_diagnostic_code_shape': ('none', 'missing', 'string_zero',
                                      'string_type_mismatch', 'string_invalid_classpath', 'other_string',
                                      'integer_zero', 'integer_type_mismatch',
                                      'other_integer', 'other'),
    'rejected_diagnostic_message_class': ('none', 'missing', 'offline_owned_dependency',
                                         'plain_missing_owned_dependency',
                                         'deliberate_int_to_string', 'unresolved_cedar_import',
                                         'unresolved_arithmetic', 'owned_missing_maven_library', 'other'),
    'rejected_diagnostic_severity': ('none', 'missing', 'error', 'warning',
                                    'information', 'hint', 'other'),
})
AGENT_TRANSCRIPT_FIELDS['windows_java_maven'] = dict.fromkeys((
    'cache_input_unchanged', 'fixture_inputs_verified', 'elapsed_saturated',
    'primary_failed', 'cleanup_failed', 'success'), 'bool') | {
        'schema_version': ('integer_range', 1, 1),
        'route': ('normal_agent_normal_client',),
        'pair_count': ('integer_range', 0, 1),
        'cache_input_files': ('integer_range', 0, 83),
        'source_sha256': ('5dda0de22c2184b1420be8e68f8a37e9165b59658d5c5cbf9fe2ee770a1003e5',),
        'pom_sha256': ('c13116c2a4d7dd73f28f604480b6aad3ce818a11db526b7f61737c1c3864b65b',),
        'dependency_jar_sha256': ('82579c654968c77f0bd3d04c28a22b24396c35270ce76d015807410438952b5d',),
        'present': MAVEN_CASE_FIELDS,
        'missing': MAVEN_CASE_FIELDS,
        'elapsed_ms': ('integer_range', 0, 360000),
    }


def is_link(info):
    """All Windows reparse points are rejected, including junctions."""
    return stat.S_ISLNK(info.st_mode) or bool(
        getattr(info, 'st_file_attributes', 0) & getattr(stat, 'FILE_ATTRIBUTE_REPARSE_POINT', 0x400))


def error_details(error):
    # str(error) can contain an absolute path or uncontrolled file contents.
    result = {'error_type': type(error).__name__}
    if getattr(error, 'errno', None) is not None:
        result['errno'] = error.errno
    return result


def check_components(path):
    """Reject links in every existing component, without resolving through one."""
    for component in reversed((path, *path.parents)):
        info = component.lstat()
        if is_link(info):
            raise ValueError('symlink_or_reparse_point')
    return info


def windows_path_key(value):
    """Normalize only Windows spelling, never follow links in this helper."""
    value = os.fspath(value)
    if value.lower().startswith('\\\\?\\unc\\'):
        value = '\\\\' + value[8:]
    elif value.startswith('\\\\?\\'):
        value = value[4:]
    return ntpath.normcase(ntpath.normpath(value))


def check_root_identity(root, root_info):
    current_root = check_components(root)
    if (current_root.st_dev, current_root.st_ino) != (root_info.st_dev, root_info.st_ino):
        raise ValueError('root_changed')


def checked_windows_canonical_path(root, path, root_info):
    """Expand 8.3 aliases only after rejecting every reparse component.

    Python documents realpath's Windows 8.3 expansion at:
    https://docs.python.org/3/library/os.path.html#os.path.realpath
    Recheck after normalization so it cannot authorize a linked input, and
    retain the original root identity instead of adopting a replacement root.
    """
    check_root_identity(root, root_info)
    check_components(path)
    root_key = windows_path_key(os.path.realpath(root, strict=True))
    path_key = windows_path_key(os.path.realpath(path, strict=True))
    if ntpath.commonpath((root_key, path_key)) != root_key:
        raise ValueError('canonical_path_outside_root')
    check_components(path)
    check_root_identity(root, root_info)
    return path_key


def windows_handle_path(fd):
    # Validate the opened handle before reading bytes, including on Python
    # versions without Path.is_junction or O_NOFOLLOW.
    import ctypes
    from ctypes import wintypes
    import msvcrt
    function = ctypes.WinDLL('kernel32', use_last_error=True).GetFinalPathNameByHandleW
    function.argtypes = (wintypes.HANDLE, wintypes.LPWSTR, wintypes.DWORD, wintypes.DWORD)
    function.restype = wintypes.DWORD
    buffer = ctypes.create_unicode_buffer(32768)
    length = function(msvcrt.get_osfhandle(fd), buffer, len(buffer), 0)
    if not length or length >= len(buffer):
        raise OSError(ctypes.get_last_error(), 'handle_path_unavailable')
    return windows_path_key(buffer.value)


def read_checked(root, path, expected, root_info, limit):
    """Read regular files only; never follow a linked file or directory."""
    check_components(path)
    flags = os.O_RDONLY | getattr(os, 'O_BINARY', 0) | getattr(os, 'O_NONBLOCK', 0)
    directories = []
    fd = None
    try:
        if os.open in os.supports_dir_fd and hasattr(os, 'O_NOFOLLOW'):
            directory_flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW
            parent = os.open(root, directory_flags)
            directories.append(parent)
            actual_root = os.fstat(parent)
            if (actual_root.st_dev, actual_root.st_ino) != (root_info.st_dev, root_info.st_ino):
                raise ValueError('root_changed')
            parts = path.relative_to(root).parts
            for part in parts[:-1]:
                parent = os.open(part, directory_flags, dir_fd=parent)
                directories.append(parent)
            fd = os.open(parts[-1], flags | os.O_NOFOLLOW, dir_fd=parent)
        else:
            canonical_path = checked_windows_canonical_path(root, path, root_info) if os.name == 'nt' else None
            fd = os.open(path, flags | getattr(os, 'O_NOFOLLOW', 0))
            if os.name == 'nt' and windows_handle_path(fd) != canonical_path:
                raise ValueError('opened_path_changed')
            check_components(path)
            check_root_identity(root, root_info)
        before = os.fstat(fd)
        if not stat.S_ISREG(before.st_mode) or is_link(before) or before.st_nlink != 1:
            raise ValueError('not_unlinked_regular_file')
        if (before.st_dev, before.st_ino, before.st_size) != (expected.st_dev, expected.st_ino, expected.st_size):
            raise ValueError('file_changed')
        with os.fdopen(fd, 'rb') as stream:
            fd = None
            data = stream.read(limit + 1)
            after = os.fstat(stream.fileno())
        if (before.st_size, before.st_mtime_ns) != (after.st_size, after.st_mtime_ns):
            raise ValueError('file_changed')
        if len(data) > limit or len(data) != before.st_size:
            raise ValueError('file_size_changed')
        return data
    finally:
        if fd is not None:
            os.close(fd)
        for directory in reversed(directories):
            os.close(directory)


def parse_frame(line):
    """Reconstruct selected fields, never copy arbitrary trailing text."""
    match = re.fullmatch(r'([JjVvC])\s+(.+)', line)
    if not match:
        return None
    kind, body = match.groups()
    result = {'kind': kind}
    if kind in 'Jj':
        # Compilation identifiers, addresses and annotations are omitted.
        method = JAVA_METHOD.match(re.sub(r'^\d+%?\s+(?:(?:c1|c2|jvmci)\s+)?', '', body))
        if not method:
            return None
        result['method'] = method.group(0)
        return result
    library = LIBRARY_FRAME.match(body)
    if library:
        # Absolute build/library locations do not belong in diagnostics.
        name = library['library'].replace('\\', '/').rsplit('/', 1)[-1]
        if not LIBRARY_NAME.fullmatch(name):
            return None
        result.update(library=name, offset=library['offset'])
        symbol = NATIVE_SYMBOL.fullmatch(body[library.end():].strip())
        if symbol:
            result['symbol'] = symbol['symbol']
        return result
    if re.fullmatch(r'0x[0-9a-fA-F]+', body):
        result['pc'] = body
        return result
    if kind == 'v' and re.fullmatch(r'~?(?:StubRoutines|RuntimeStub)::[A-Za-z0-9_]+', body):
        result['stub'] = body
        return result
    return None


def parse_evidence(data, limits):
    # ACP/non-UTF-8 bytes in omitted Windows paths or environment values must
    # not discard otherwise useful ASCII crash fields. No decoded raw text is
    # returned; each retained string still passes its own narrow grammar.
    text = data.decode('utf-8', errors='replace')
    if '\x00' in text:
        raise ValueError('binary_log')
    evidence = {'fatal_error': False, 'headers': {}, 'problematic_frame': [],
                'native_frames': [], 'java_frames': []}
    truncated = set()
    omitted_frames = 0
    section = None
    header_block = True
    for raw_line in text.splitlines():
        if len(raw_line) > limits['line_characters']:
            truncated.add('line_characters_limit')
            section = None
            continue
        line = raw_line.strip()
        if header_block and line.startswith('#'):
            header = line[1:].strip()
            if header == FATAL_HEADER:
                evidence['fatal_error'] = True
            exception = EXCEPTION.fullmatch(header)
            if exception:
                evidence['exception'] = {key: value for key, value in exception.groupdict().items() if value is not None}
            elif re.fullmatch(r'Internal Error \([^\r\n]*\), pid=\d+, tid=\d+', header):
                # Internal-error source locations and assertion text are omitted.
                evidence['exception'] = {'name': 'Internal Error'}
            for prefix, key, names in (
                ('JRE version: ', 'jre', ('OpenJDK Runtime Environment', 'Java(TM) SE Runtime Environment')),
                ('Java VM: ', 'vm', ('OpenJDK 64-Bit Server VM', 'OpenJDK Server VM', 'OpenJDK Client VM',
                                   'Java HotSpot(TM) 64-Bit Server VM', 'Java HotSpot(TM) Server VM', 'Java HotSpot(TM) Client VM')),
            ):
                if header.startswith(prefix):
                    value = header[len(prefix):]
                    name = next((item for item in names if value.startswith(item)), None)
                    if name:
                        details = {'name': name}
                        build = re.search(r'\(build ([^\s()]+)\)', value)
                        version = VERSION.fullmatch(build[1]) if build else VERSION.search(value[len(name):])
                        if version:
                            details['version'] = version[0]
                        evidence['headers'][key] = details
            if header == 'Problematic frame:':
                section = 'problematic_frame'
            elif section == 'problematic_frame' and header:
                frame = parse_frame(header)
                if frame:
                    if len(evidence[section]) < limits['frames_per_section']:
                        evidence[section].append(frame)
                    else:
                        truncated.add(section + '_limit')
                else:
                    omitted_frames += 1
                section = None
            continue
        if not line:
            section = None
            continue
        header_block = False
        if re.fullmatch(r'Native frames:(?: \([^\r\n]*\))?', line):
            section = 'native_frames'
            continue
        if re.fullmatch(r'Java frames:(?: \([^\r\n]*\))?', line):
            section = 'java_frames'
            continue
        if section in ('native_frames', 'java_frames'):
            if line.startswith(('J ', 'j ', 'V ', 'v ', 'C ')):
                frame = parse_frame(line)
                if frame is None:
                    omitted_frames += 1
                elif len(evidence[section]) < limits['frames_per_section']:
                    evidence[section].append(frame)
                else:
                    truncated.add(section + '_limit')
            else:
                # A register dump, stack memory or another section ends capture.
                section = None
    if not evidence['fatal_error'] or not (evidence.get('exception') or evidence['problematic_frame']):
        raise ValueError('unrecognized_fatal_log')
    evidence['omitted_unrecognized_frames'] = omitted_frames
    return evidence, sorted(truncated)


def safe_fields(value, schema, errors):
    """Reconstruct declared scalars and fixed nested schemas; ignore other data."""
    result = {}
    for key, kind in schema.items():
        if key not in value:
            continue
        item = value[key]
        if isinstance(kind, dict):
            if isinstance(item, dict):
                result[key] = safe_fields(item, kind, errors)
            else:
                errors.add('invalid_field_' + key)
            continue
        nullable = isinstance(kind, str) and kind.startswith('?')
        if nullable:
            kind = kind[1:]
        if item is None and nullable:
            valid = True
        elif kind == 'bool':
            valid = type(item) is bool
        elif kind in ('count', 'u32', 'u64'):
            maximum = {'count': 2 ** 53 - 1, 'u32': 2 ** 32 - 1, 'u64': 2 ** 64 - 1}[kind]
            valid = type(item) is int and 0 <= item <= maximum
        elif isinstance(kind, tuple) and kind and kind[0] == 'integer_range':
            valid = type(item) is int and kind[1] <= item <= kind[2]
        elif isinstance(kind, tuple) and kind and kind[0] == 'enum_list':
            valid = (isinstance(item, list) and len(item) <= len(kind[1])
                     and all(isinstance(part, str) and part in kind[1] for part in item))
        elif isinstance(kind, tuple) and kind and kind[0] == 'nullable_enum':
            valid = item is None or isinstance(item, str) and item in kind[1]
        else:
            valid = isinstance(item, str) and item in kind
        if valid:
            result[key] = item
        else:
            # The field name comes from our schema, never from untrusted keys.
            errors.add('invalid_field_' + key)
    return result


def fatal_from_prefix(value, limits, errors, truncation):
    """Find a fatal header in a private byte prefix; return reconstructed fields."""
    maximum = limits['probe_stream_bytes']
    raw = value.get('prefix_hex')
    data = None
    if 'prefix_hex' in value and not isinstance(raw, str):
        errors.add('invalid_prefix_hex')
    if isinstance(raw, str):
        if len(raw) > maximum * 2:
            truncation.add('probe_stream_bytes_limit')
        raw = raw[:maximum * 2]
        if len(raw) % 2 or not re.fullmatch(r'[0-9a-fA-F]*', raw):
            errors.add('invalid_prefix_hex')
        else:
            data = bytes.fromhex(raw)
    if data is None and isinstance(value.get('prefix_utf8_lossy'), str):
        raw = value['prefix_utf8_lossy']
        if len(raw) > maximum:
            truncation.add('probe_stream_bytes_limit')
        data = raw[:maximum].encode('utf-8', errors='replace')[:maximum]
    if data is None:
        return None
    text = data.decode('utf-8', errors='replace')
    lines = text.splitlines()
    start = next((index for index, line in enumerate(lines)
                  if re.fullmatch(r'#\s*' + re.escape(FATAL_HEADER), line.strip())), None)
    if start is None:
        return None
    try:
        evidence, bounded = parse_evidence('\n'.join(lines[start:]).encode('utf-8'), limits)
        truncation.update(bounded)
        return evidence
    except ValueError:
        errors.add('incomplete_fatal_prefix')
        return {'fatal_error': True, 'status': 'error', 'reason': 'incomplete_fatal_prefix'}


def sanitize_probe(data, limits):
    source = json.loads(data.decode('utf-8-sig'))
    if not isinstance(source, dict):
        raise ValueError('probe_report_not_object')
    errors, truncation = set(), set()
    report = safe_fields(source, {
        'driver_status': ('diagnostic_only', 'setup_error', 'unsupported_platform'),
        'driver_completed': 'bool', 'acceptance_claimed': 'bool', 'case_count': 'count',
    }, errors)
    if not all(key in report for key in ('driver_status', 'driver_completed', 'acceptance_claimed')):
        errors.add('missing_probe_driver_metadata')
    report['cases'] = []
    cases = source.get('cases', [])
    if not isinstance(cases, list):
        raise ValueError('probe_cases_not_array')
    if len(cases) > limits['probe_cases']:
        truncation.add('probe_cases_limit')
    if 'case_count' in report and report['case_count'] != len(cases):
        errors.add('case_count_mismatch')
    seen = set()
    for case in cases[:limits['probe_cases']]:
        if not isinstance(case, dict) or case.get('name') not in PROBE_CASES:
            errors.add('unknown_probe_case')
            continue
        name = case['name']
        if name in seen:
            errors.add('duplicate_probe_case')
        seen.add(name)
        result = safe_fields(case, {
            'name': PROBE_CASES, 'outcome': PROBE_OUTCOMES, 'exit_code': '?u32',
            'job_active_processes_final': '?u32', 'stdin_expected_bytes': 'count',
            'stdin_accepted_bytes': 'count',
            **dict.fromkeys(('job_zero_observed', 'cleanup_verified', 'root_joined',
                             'root_exit_observed_before_termination', 'capture_cancellation_completed',
                             'stdin_cancellation_completed', 'stdin_eof_sent_before_cleanup', 'timed_out'), 'bool'),
        }, errors)
        if 'outcome' not in result:
            errors.add('missing_probe_outcome')
        if type(result.get('exit_code')) is int:
            result['exit_code_hex'] = f"0x{result['exit_code']:08X}"
        if isinstance(case.get('hello_markers'), dict):
            result['hello_markers'] = safe_fields(case['hello_markers'], dict.fromkeys(
                ('stdout_ready', 'stderr_ready', 'stdin_echo', 'stdout_done', 'stderr_done'), 'bool'), errors)
        elif 'hello_markers' in case:
            errors.add('invalid_hello_markers')
        for stream in ('stdout', 'stderr'):
            value = case.get(stream)
            if value is None:
                continue
            if not isinstance(value, dict):
                errors.add('invalid_' + stream)
                continue
            capture = safe_fields(value, {
                'observed_bytes': 'count', 'retained_bytes': 'count',
                'retention_limit_bytes': 'count', 'truncated': 'bool', 'eof_observed': 'bool',
            }, errors)
            if capture.get('truncated'):
                truncation.add('probe_capture_truncated')
            fatal = fatal_from_prefix(value, limits, errors, truncation)
            capture['fatal_header_observed'] = fatal is not None
            if fatal is not None:
                capture['fatal_evidence'] = fatal
            result[stream] = capture
        report['cases'].append(result)
    return report, errors, truncation


def sanitize_transcript(data, limits, schema=TRANSCRIPT_FIELDS):
    errors, truncation = set(), set()
    report = {'records': [], 'omitted_non_json_lines': 0, 'omitted_other_json_records': 0}
    idle_seen = False
    implementations_seen = False
    # Each private source has a separate whitelist; arbitrary payloads never
    # become public evidence even when they appear alongside a known record.
    for line in data.decode('utf-8-sig', errors='replace').splitlines():
        if not line.strip():
            continue
        if len(line) > limits['transcript_line_characters']:
            truncation.add('transcript_line_characters_limit')
            continue
        duplicate_keys = False

        def object_pairs(pairs):
            nonlocal duplicate_keys
            value = {}
            for key, item in pairs:
                if key in value:
                    duplicate_keys = True
                value[key] = item
            return value

        try:
            value = json.loads(line, object_pairs_hook=object_pairs)
        except (ValueError, RecursionError):
            if line.lstrip().startswith('{'):
                errors.add('malformed_json_line')
            report['omitted_non_json_lines'] += 1
            continue
        if not isinstance(value, dict) or not isinstance(value.get('kind'), str) or value['kind'] not in schema:
            report['omitted_other_json_records'] += 1
            continue
        if len(report['records']) >= limits['transcript_records']:
            truncation.add('transcript_records_limit')
            continue
        kind = value['kind']
        if kind == 'windows_java_idle_correction':
            if idle_seen:
                errors.add('duplicate_idle_receipt')
            idle_seen = True
            if duplicate_keys:
                errors.add('duplicate_idle_receipt_field')
                continue
            for field in schema[kind]:
                if field not in value:
                    errors.add('missing_field_' + field)
        if kind == 'windows_java_implementations':
            if implementations_seen:
                errors.add('duplicate_implementations_receipt')
            implementations_seen = True
            if duplicate_keys:
                errors.add('duplicate_implementations_receipt_field')
                continue
            for field in schema[kind]:
                if field not in value:
                    errors.add('missing_field_' + field)
        record = {'kind': kind} | safe_fields(value, schema[kind], errors)
        if kind == 'windows_java_gc_control' and 'route' not in value:
            errors.add('missing_gc_control_route')
        if 'session' in schema[kind] and 'session' not in record:
            errors.add('missing_session')
        report['records'].append(record)
    return report, errors, truncation


def sanitize_agent_transcript(data, limits):
    return sanitize_transcript(data, limits, AGENT_TRANSCRIPT_FIELDS)


def collect_source(root, source_path, root_info, limits, sanitizer):
    """Optional private inputs have the same path and read boundary as logs."""
    item = {'status': 'error', 'bytes': None, 'sha256': None, 'truncated': False}
    source_path = Path(source_path)
    path = Path(os.path.abspath(source_path if source_path.is_absolute() else root / source_path))
    try:
        relative = path.relative_to(root)
    except ValueError:
        item['reason'] = 'source_outside_root'
        return item
    item['relative_filename'] = relative.as_posix()
    try:
        info = check_components(path)
        item['bytes'] = info.st_size
        if not stat.S_ISREG(info.st_mode) or is_link(info) or info.st_nlink != 1:
            item.update(status='skipped', reason='not_unlinked_regular_file')
            return item
        if info.st_size > limits['source_file_bytes']:
            item.update(status='skipped', reason='source_file_bytes_limit')
            return item
        data = read_checked(root, path, info, root_info, limits['source_file_bytes'])
        item['sha256'] = hashlib.sha256(data).hexdigest()
        evidence, errors, truncation = sanitizer(data, limits)
        item.update(status='error' if errors else ('truncated' if truncation else 'collected'),
                    truncated=bool(truncation), evidence=evidence)
        if errors:
            item['errors'] = sorted(errors)
        if truncation:
            item['truncation_reasons'] = sorted(truncation)
    except (OSError, ValueError, RecursionError) as error:
        item.update(status='error', reason='source_read_or_parse_failed', **error_details(error))
    return item


def collect(root, limits=None, probe_report=None, java_transcript=None, agent_transcript=None):
    limits = dict(LIMITS if limits is None else limits)
    root = Path(os.path.abspath(root))
    report = {'schema_version': 1, 'purpose': 'sanitized_jvm_crash_diagnostics',
              'acceptance_result': 'not_evaluated', 'status': 'complete',
              'limits': limits, 'files': [], 'issues': []}

    def issue(status, reason, path=None, error=None):
        item = {'status': status, 'reason': reason}
        if path is not None:
            item['relative_filename'] = path.relative_to(root).as_posix()
        if error is not None:
            item.update(error_details(error))
        report['issues'].append(item)

    try:
        root_info = check_components(root)
        if not stat.S_ISDIR(root_info.st_mode):
            raise ValueError('root_not_directory')
    except (OSError, ValueError) as error:
        issue('error', 'unsafe_or_unavailable_root', error=error)
        report['status'] = 'error'
        return report

    entries_seen = 0
    stopped = False

    def visit(directory, depth):
        nonlocal entries_seen, stopped
        try:
            check_components(directory)
            with os.scandir(directory) as entries:
                for entry in entries:
                    if stopped:
                        break
                    if entries_seen >= limits['directory_entries']:
                        issue('truncated', 'directory_entries_limit')
                        stopped = True
                        break
                    entries_seen += 1
                    # Filter before metadata or any path-bearing issue. Even a
                    # dangling link, reparse point, unreadable entry or directory
                    # in this namespace must not reveal the JVM PID indirectly.
                    if entry.name.lower().startswith(GC_PRIVATE_PREFIX):
                        continue
                    path = directory / entry.name
                    matches = bool(LOG_NAME.fullmatch(entry.name))
                    try:
                        # DirEntry.stat caches metadata, and on Windows its
                        # st_ino/st_dev/st_nlink are always zero. Fresh lstat
                        # supplies real identity/link counts without following
                        # symlinks or junctions. Never accept zero as one link.
                        # https://docs.python.org/3/library/os.html#os.DirEntry.stat
                        info = path.lstat()
                    except OSError as error:
                        issue('error', 'stat_failed', path, error)
                        continue
                    if is_link(info):
                        issue('skipped', 'symlink_or_reparse_point', path)
                        continue
                    if stat.S_ISDIR(info.st_mode):
                        if depth >= limits['depth']:
                            issue('truncated', 'depth_limit', path)
                        else:
                            visit(path, depth + 1)
                        continue
                    if not matches:
                        continue
                    if len(report['files']) >= limits['files']:
                        issue('truncated', 'file_count_limit')
                        stopped = True
                        break
                    item = {'relative_filename': path.relative_to(root).as_posix(),
                            'bytes': info.st_size, 'sha256': None, 'status': 'skipped'}
                    report['files'].append(item)
                    if not stat.S_ISREG(info.st_mode):
                        item['reason'] = 'not_regular_file'
                    elif info.st_nlink != 1:
                        item['reason'] = 'hard_link'
                    elif info.st_size > limits['file_bytes']:
                        item['reason'] = 'file_bytes_limit'
                    else:
                        try:
                            data = read_checked(root, path, info, root_info, limits['file_bytes'])
                            item['sha256'] = hashlib.sha256(data).hexdigest()
                            evidence, truncation = parse_evidence(data, limits)
                            item.update(status='truncated' if truncation else 'collected', evidence=evidence)
                            if truncation:
                                item['reasons'] = truncation
                        except (OSError, ValueError) as error:
                            item.update(status='error', reason='read_or_parse_failed', **error_details(error))
        except (OSError, ValueError) as error:
            issue('error', 'directory_scan_failed', directory, error)

    visit(root, 0)
    report['files'].sort(key=lambda item: item['relative_filename'])
    sources = []
    for key, path, sanitizer in (('probe_report', probe_report, sanitize_probe),
                                 ('java_transcript', java_transcript, sanitize_transcript),
                                 ('agent_transcript', agent_transcript, sanitize_agent_transcript)):
        if path is not None:
            report[key] = collect_source(root, path, root_info, limits, sanitizer)
            sources.append(report[key])
    statuses = {item['status'] for item in report['files'] + report['issues'] + sources}
    if 'error' in statuses:
        report['status'] = 'error'
    elif statuses.intersection({'skipped', 'truncated'}):
        report['status'] = 'partial'
    report['directory_entries_scanned'] = entries_seen
    return report


def write_report(path, report):
    path = Path(os.path.abspath(path))
    if LOG_NAME.fullmatch(path.name):
        raise ValueError('output_must_not_be_a_crash_log')
    check_components(path.parent)
    if path.exists() or path.is_symlink():
        info = path.lstat()
        if is_link(info) or not stat.S_ISREG(info.st_mode) or info.st_nlink != 1:
            raise ValueError('unsafe_output')
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(mode='w', encoding='utf-8', newline='\n',
                                         dir=path.parent, prefix='.java-crash-', suffix='.json', delete=False) as stream:
            temporary = Path(stream.name)
            json.dump(report, stream, indent=2, ensure_ascii=True)
            stream.write('\n')
        os.replace(temporary, path)
    finally:
        if temporary is not None and temporary.exists():
            temporary.unlink()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, required=True, help='Generated CI scratch directory only')
    parser.add_argument('--output', type=Path, required=True, help='Sanitized JSON report; parent must exist')
    parser.add_argument('--probe-report', type=Path, help='Private probe JSON stdout file inside root')
    parser.add_argument('--java-transcript', type=Path, help='Private real-Java output file inside root')
    parser.add_argument('--agent-transcript', type=Path, help='Private headless-agent output file inside root')
    args = parser.parse_args()
    report = collect(args.root, probe_report=args.probe_report, java_transcript=args.java_transcript,
                     agent_transcript=args.agent_transcript)
    try:
        write_report(args.output, report)
    except (OSError, ValueError) as error:
        print(json.dumps({'status': 'error', 'reason': 'report_write_failed', **error_details(error)}), file=sys.stderr)
        return 1
    print(json.dumps({'status': report['status'], 'files': len(report['files']),
                      'issues': len(report['issues']), 'acceptance_result': 'not_evaluated'}))
    return 0 if report['status'] == 'complete' else 1


if __name__ == '__main__':
    sys.exit(main())
