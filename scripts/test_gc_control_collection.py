#!/usr/bin/env python3
"""Synthetic-only privacy, parser and ownership-boundary GC collector tests."""
import hashlib
import json
import os
from pathlib import Path
import stat
import subprocess
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock

import collect_gc_control as collector


HEADER = b'[8ms][info][gc] Using G1\n'
EVENT = b'[1234ms][info][gc] GC(0) Pause Young (Normal) (G1 Evacuation Pause) 31M->3M(128M) 2.037ms\n'
SAMPLE = HEADER + EVENT
SECRET = 'SECRET_PATH_PID_ENV_ADDRESS_ERROR_SENTINEL'


class GCCollectionTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='cedar-gc-synthetic-')
        self.addCleanup(self.temporary.cleanup)
        self.base = Path(self.temporary.name)
        self.root = self.base / 'owned'
        self.root.mkdir()
        self.selection_path = self.root / 'private-selection.json'
        self.witness = {
            'schema_version': 1, 'kind': 'cedar_gc_control_selection', 'pid': 314,
            'creation_time_100ns_since_1601': 133000000000000000,
            'root_exit_code': 0,
            **dict.fromkeys(collector.SELECTION_TRUE_FIELDS, True),
        }
        self.write_selection()

    def write_selection(self, content=None):
        self.selection_path.write_text(json.dumps(self.witness if content is None else content),
                                       encoding='utf-8')

    def log(self, data=SAMPLE, suffix='', pid=314):
        path = self.root / ('cedar-gc-' + str(pid) + '.log' + suffix)
        path.write_bytes(data)
        return path

    def collect(self):
        result = collector.collect(self.root, self.selection_path)
        encoded = json.dumps(result, allow_nan=False)
        for private in (SECRET, str(self.root), 'cedar-gc-314', '133000000000000000'):
            self.assertNotIn(private, encoded)
        return result

    def parse(self, data=SAMPLE, **limit_changes):
        return collector.parse_logs([('active', data)], dict(collector.LIMITS, **limit_changes))

    def link(self, target, path, directory=False):
        try:
            path.symlink_to(target, target_is_directory=directory)
        except (OSError, NotImplementedError):
            self.skipTest('Synthetic symlink creation is unavailable.')

    def test_numeric_schema_and_whole_file_digest(self):
        data = SAMPLE.replace(b'\n', b'\r\n')
        self.log(data)
        report = self.collect()
        self.assertEqual(report['status'], 'complete')
        self.assertEqual(report['collector'], 'g1')
        self.assertTrue(report['diagnostic_only'])
        self.assertEqual(report['acceptance_result'], 'not_evaluated')
        self.assertEqual(report['heap_observation'], 'observed_gc_points')
        self.assertEqual(report['heap_value_semantics'], 'gc_point_floor_mib')
        self.assertEqual(report['heap_value_quantum_bytes'], 1048576)
        self.assertEqual(report['files'], [{'slot': 'active', 'bytes': len(data),
                                          'sha256': hashlib.sha256(data).hexdigest()}])
        self.assertEqual(report['events'], [{
            'uptime_ms': 1234, 'gc_id': 0, 'event_kind': 'pause_young', 'pause_ms': 2.037,
            'used_before_bytes': 31 * collector.MIB, 'used_after_bytes': 3 * collector.MIB,
            'heap_capacity_bytes': 128 * collector.MIB,
        }])
        self.assertEqual(set(report), {
            'schema_version', 'diagnostic_only', 'acceptance_result', 'status', 'issues',
            'selection_binding_verified', 'selection_sha256',
            'collector', 'collector_header_count', 'heap_observation', 'heap_value_semantics',
            'heap_value_quantum_bytes', 'pause_precision_ms', 'files', 'events',
            'lines_examined', 'omitted_lines',
        })

    def test_zero_logged_mib_is_quantized_not_exact_zero(self):
        report = self.parse(SAMPLE.replace(b'3M(128M)', b'0M(128M)'))
        self.assertEqual(report['events'][0]['used_after_bytes'], 0)
        self.assertEqual(report['heap_value_semantics'], 'gc_point_floor_mib')
        self.assertEqual(report['heap_value_quantum_bytes'], collector.MIB)

    def test_capacity_is_after_gc_and_can_be_less_than_usage_before_gc(self):
        report = self.parse(SAMPLE.replace(b'31M->3M(128M)', b'300M->3M(64M)'))
        self.assertEqual(report['status'], 'complete')
        self.assertEqual(report['events'][0]['heap_capacity_bytes'], 64 * collector.MIB)
        self.assertGreater(report['events'][0]['used_before_bytes'],
                           report['events'][0]['heap_capacity_bytes'])

    def test_unsupported_units_decimals_and_nonfinite_numbers_are_not_guessed(self):
        for replacement in (b'31K->3K(128K)', b'31G->3G(128G)', b'31.5M->3M(128M)',
                            b'NaNM->3M(128M)', b'-31M->3M(128M)'):
            with self.subTest(replacement=replacement):
                report = self.parse(SAMPLE.replace(b'31M->3M(128M)', replacement))
                self.assertEqual(report['status'], 'partial')
                self.assertEqual(report['events'], [])
        for duration in (b'NaN', b'Infinity', b'-1.000', b'1e300', b'1.0000'):
            self.assertEqual(self.parse(SAMPLE.replace(b'2.037', duration))['events'], [])

    def test_out_of_range_and_impossible_after_capacity_are_omitted(self):
        for before_after in (b'31M->129M(128M)', b'31M->0M(0M)',
                             b'9007199254740991M->3M(128M)'):
            report = self.parse(SAMPLE.replace(b'31M->3M(128M)', before_after))
            self.assertIn('numeric_range', report['issues'])
            self.assertEqual(report['events'], [])
        report = self.parse(SAMPLE.replace(b'[1234ms]', b'[9007199254740992ms]'))
        self.assertIn('numeric_range', report['issues'])
        self.assertEqual(report['events'], [])

    def test_known_collector_without_gc_is_not_zero_heap(self):
        for name, enum in ((b'G1', 'g1'), (b'Serial', 'serial'), (b'Parallel', 'parallel')):
            report = self.parse(HEADER.replace(b'G1', name))
            self.assertEqual(report['status'], 'complete')
            self.assertEqual(report['collector'], enum)
            self.assertEqual(report['heap_observation'], 'not_observed')
            self.assertEqual(report['events'], [])

    def test_unknown_missing_repeated_or_late_collector_header(self):
        for data, issue in ((EVENT, 'collector_header_missing'),
                            (SAMPLE.replace(b'Using G1', ('Using ' + SECRET).encode()), 'collector_unknown'),
                            (SAMPLE + HEADER, 'collector_header_repeated'),
                            (SAMPLE.replace(b'[8ms]', b'[1235ms]'), 'collector_header_order')):
            report = self.parse(data)
            self.assertIn(issue, report['issues'])
            self.assertEqual(report['events'], [])
            self.assertNotIn(SECRET, json.dumps(report))
        self.assertEqual(self.parse(EVENT)['collector'], 'unknown')

    def test_supported_identity_with_unsupported_heap_format_is_explicit(self):
        for name, enum in ((b'ZGC', 'zgc'), (b'Shenandoah', 'shenandoah'), (b'Epsilon', 'epsilon')):
            report = self.parse(SAMPLE.replace(b'Using G1', b'Using ' + name))
            self.assertEqual(report['collector'], enum)
            self.assertIn('collector_heap_format_unsupported', report['issues'])
            self.assertEqual(report['events'], [])

    def test_three_file_rotation_order_uses_uptime(self):
        self.log(EVENT.replace(b'[1234ms]', b'[5000ms]').replace(b'GC(0)', b'GC(2)'))
        self.log(HEADER + EVENT, '.0')
        self.log(EVENT.replace(b'[1234ms]', b'[2500ms]').replace(b'GC(0)', b'GC(1)'), '.1')
        report = self.collect()
        self.assertEqual(report['status'], 'complete')
        self.assertEqual(len(report['files']), 3)
        self.assertEqual([event['uptime_ms'] for event in report['events']], [1234, 2500, 5000])

    def test_header_rotation_loss_is_explicit(self):
        self.log(EVENT)
        report = self.collect()
        self.assertEqual(report['collector'], 'unknown')
        self.assertEqual(report['events'], [])
        self.assertIn('collector_header_missing', report['issues'])

    def test_only_owned_pid_and_three_exact_names_are_opened(self):
        self.log()
        self.log((SECRET + '\n').encode(), pid=999)
        self.log((SECRET + '\n').encode(), suffix='.2')
        self.log((SECRET + '\n').encode(), suffix='.0.extra')
        nested = self.root / 'nested'
        nested.mkdir()
        (nested / 'cedar-gc-314.log').write_text(SECRET)
        with mock.patch.object(collector, 'read_checked', wraps=collector.read_checked) as read:
            report = self.collect()
        self.assertEqual(report['status'], 'complete')
        self.assertEqual(len(report['files']), 1)
        self.assertEqual({call.args[1] for call in read.call_args_list},
                         {self.selection_path, self.root / 'cedar-gc-314.log'})

    def test_sensitive_lines_headers_causes_and_errors_never_escape(self):
        poison = ('Command Line: ' + SECRET + '\n' +
                  '[2ms][error][os] ' + SECRET + '\n' +
                  '[3ms][info][gc] ' + SECRET + '\n').encode()
        data = HEADER + poison + EVENT.replace(b'(Normal)', ('(' + SECRET + ')').encode())
        self.log(data)
        report = self.collect()
        self.assertNotIn(SECRET, json.dumps(report))
        self.assertEqual(len(report['events']), 1)
        with mock.patch.object(collector, 'read_checked', side_effect=OSError(SECRET)):
            self.assertEqual(self.collect()['status'], 'rejected')

    def test_trailing_private_text_cannot_extend_valid_numeric_record(self):
        report = self.parse(SAMPLE[:-1] + b' ' + SECRET.encode() + b'\n')
        self.assertEqual(report['events'], [])
        self.assertNotIn(SECRET, json.dumps(report))

    def test_truncated_last_line_is_not_an_observation(self):
        report = self.parse(SAMPLE[:-1])
        self.assertIn('unterminated_line', report['issues'])
        self.assertEqual(report['events'], [])

    def test_bounds_for_bytes_files_lines_and_events(self):
        self.assertIn('file_bytes_limit', self.parse(file_bytes=8)['issues'])
        self.assertIn('total_bytes_limit', self.parse(total_bytes=8)['issues'])
        self.assertIn('line_bytes_limit', self.parse(line_bytes=40)['issues'])
        self.assertIn('line_count_limit', self.parse(lines=1)['issues'])
        self.assertIn('event_count_limit', self.parse(events=0)['issues'])
        self.assertIn('file_count_limit', collector.parse_logs([('active', SAMPLE)] * 4)['issues'])
        self.log(b'0' * (collector.LIMITS['file_bytes'] + 1))
        self.assertEqual(self.collect()['issues'], ['file_bytes_limit'])

    def test_concurrent_cycle_duration_is_not_a_pause(self):
        report = self.parse(SAMPLE + b'[1300ms][info][gc] GC(1) Concurrent Mark Cycle\n'
                            b'[1600ms][info][gc] GC(1) Concurrent Mark Cycle 300.456ms\n')
        self.assertEqual(report['status'], 'complete')
        self.assertEqual(len(report['events']), 1)

    def test_natural_log_can_report_system_gc_without_collector_invoking_it(self):
        report = self.parse(HEADER + b'[2000ms][info][gc] GC(0) Pause Full (System.gc()) '
                            b'64M->4M(64M) 10.001ms\n')
        self.assertEqual(report['status'], 'complete')
        self.assertEqual(report['events'][0]['event_kind'], 'pause_full')
        self.assertEqual(report['events'][0]['pause_ms'], 10.001)

    def test_duplicate_records_and_backwards_timestamps_are_incomplete(self):
        self.assertIn('duplicate_event', self.parse(SAMPLE + EVENT)['issues'])
        report = self.parse(SAMPLE + EVENT.replace(b'[1234ms]', b'[1000ms]'))
        self.assertIn('timestamp_order', report['issues'])

    def test_missing_active_and_all_logs(self):
        report = self.collect()
        self.assertIn('logs_missing', report['issues'])
        self.assertIn('active_log_missing', report['issues'])
        self.log(SAMPLE, '.0')
        report = self.collect()
        self.assertEqual(report['status'], 'partial')
        self.assertIn('active_log_missing', report['issues'])

    def test_selection_requires_complete_strict_owned_exit_witness(self):
        self.log()
        for key in self.witness:
            value = dict(self.witness)
            del value[key]
            self.write_selection(value)
            with self.subTest(key=key):
                self.assertEqual(self.collect()['issues'], ['selection_rejected'])
        for key, value in (('pid', True), ('pid', 0), ('pid', 1 << 32),
                           ('creation_time_100ns_since_1601', 0), ('root_exit_code', False),
                           ('root_exit_code', 1), ('natural_shutdown_verified', False),
                           ('root_image_verified', 1), ('schema_version', True),
                           ('kind', SECRET), ('extra_' + SECRET, SECRET)):
            self.write_selection(dict(self.witness, **{key: value}))
            with self.subTest(key=key, value=value):
                self.assertEqual(self.collect()['issues'], ['selection_rejected'])

    def test_malformed_duplicate_and_oversized_selection(self):
        for data in ('{"pid":314,"pid":999}', '{' + SECRET, '[' * 1500,
                     SECRET * collector.LIMITS['selection_bytes']):
            self.selection_path.write_text(data)
            self.assertEqual(self.collect()['issues'], ['selection_rejected'])

    def test_selection_must_be_inside_owned_root(self):
        outside = self.base / 'outside.json'
        outside.write_text(json.dumps(self.witness))
        for path in (outside, Path('../outside.json')):
            self.assertEqual(collector.collect(self.root, path)['issues'], ['selection_rejected'])
        self.log()
        self.assertEqual(collector.collect(self.root, 'private-selection.json')['status'], 'complete')

    def test_linked_log_selection_root_and_component_are_rejected(self):
        outside = self.base / 'outside.log'
        outside.write_bytes(SAMPLE)
        selected = self.root / 'cedar-gc-314.log'
        self.link(outside, selected)
        self.assertEqual(self.collect()['status'], 'rejected')
        selected.unlink()
        self.selection_path.unlink()
        self.link(outside, self.selection_path)
        self.assertEqual(self.collect()['issues'], ['selection_rejected'])
        root_link = self.base / 'root-link'
        self.link(self.root, root_link, directory=True)
        self.assertEqual(collector.collect(root_link, 'private-selection.json')['issues'], ['root_rejected'])

    def test_hardlinked_and_nonregular_logs_rejected(self):
        outside = self.base / 'outside.log'
        outside.write_bytes(SAMPLE)
        selected = self.root / 'cedar-gc-314.log'
        try:
            os.link(outside, selected)
        except (OSError, NotImplementedError):
            self.skipTest('Synthetic hardlink creation unavailable.')
        self.assertEqual(self.collect()['status'], 'rejected')
        selected.unlink()
        selected.mkdir()
        self.assertEqual(self.collect()['status'], 'rejected')

    def test_windows_reparse_detection_uses_shared_safe_primitive(self):
        info = SimpleNamespace(st_mode=stat.S_IFREG, st_file_attributes=0x400)
        self.assertTrue(collector.is_link(info))

    def test_file_replacement_after_checked_read_rejects_all_evidence(self):
        path = self.log()
        original = collector.read_checked

        def replace(root, selected, expected, root_info, limit):
            data = original(root, selected, expected, root_info, limit)
            if selected == path:
                selected.unlink()
                selected.write_bytes(data)
            return data

        with mock.patch.object(collector, 'read_checked', side_effect=replace):
            report = self.collect()
        self.assertEqual(report['status'], 'rejected')
        self.assertEqual(report['files'], [])
        self.assertEqual(report['events'], [])

    def test_mutation_during_actual_read_is_rejected(self):
        self.log()
        original = os.fstat
        calls = 0

        def mutate(fd):
            nonlocal calls
            info = original(fd)
            if stat.S_ISREG(info.st_mode) and info.st_size == len(SAMPLE):
                calls += 1
                if calls == 2:
                    attributes = {name: getattr(info, name) for name in dir(info) if name.startswith('st_')}
                    attributes['st_mtime_ns'] += 1
                    return SimpleNamespace(**attributes)
            return info

        with mock.patch.object(os, 'fstat', side_effect=mutate):
            self.assertEqual(self.collect()['status'], 'rejected')

    def test_new_rotation_after_initial_selection_rejected(self):
        path = self.log()
        original = collector.check_components
        calls = 0

        def add_rotation(selected):
            nonlocal calls
            result = original(selected)
            if selected == path:
                calls += 1
                if calls == 3:
                    self.log(SAMPLE, '.0')
            return result

        with mock.patch.object(collector, 'check_components', side_effect=add_rotation):
            self.assertEqual(self.collect()['status'], 'rejected')

    def test_cli_errors_do_not_echo_input_or_exception_text(self):
        script = Path(collector.__file__)
        for args in (['--' + SECRET], ['--owned-root', str(self.root), '--selection', SECRET]):
            process = subprocess.run([sys.executable, str(script), *args], capture_output=True, text=True)
            self.assertNotEqual(process.returncode, 0)
            self.assertNotIn(SECRET, process.stdout + process.stderr)
            self.assertNotIn(str(self.root), process.stdout + process.stderr)
            self.assertEqual(json.loads(process.stdout)['status'], 'rejected')

    def selection_digest(self):
        return hashlib.sha256(self.selection_path.read_bytes()).hexdigest()

    def test_bound_selection_digest_matches_exact_validated_bytes(self):
        self.log()
        digest = self.selection_digest()
        selection, path, info, actual_digest = collector.read_selection(
            self.root, self.selection_path, self.root.lstat(), collector.LIMITS)
        self.assertEqual(selection, self.witness)
        self.assertEqual(path, self.selection_path)
        self.assertEqual(info.st_size, len(self.selection_path.read_bytes()))
        self.assertEqual(actual_digest, digest)
        report = collector.collect(self.root, self.selection_path, digest)
        self.assertEqual(report['status'], 'complete')
        self.assertTrue(report['selection_binding_verified'])
        self.assertEqual(report['selection_sha256'], digest)
        unbound = self.collect()
        self.assertFalse(unbound['selection_binding_verified'])
        self.assertIsNone(unbound['selection_sha256'])

    def test_wrong_or_invalid_digest_is_rejected_before_gc_file_reads(self):
        self.log()
        for digest in ('0' * 64, 'A' * 64, 'a' * 63, 'a' * 65, SECRET, True):
            with self.subTest(digest=digest), mock.patch.object(
                    collector, 'read_checked', wraps=collector.read_checked) as read:
                report = collector.collect(self.root, self.selection_path, digest)
                self.assertEqual(report['status'], 'rejected')
                self.assertFalse(report['selection_binding_verified'])
                self.assertEqual(report['events'], [])
                self.assertEqual(report['files'], [])
                self.assertNotIn(SECRET, json.dumps(report))
                self.assertTrue(all(call.args[1] == self.selection_path for call in read.call_args_list))

    def test_replaced_valid_witness_cannot_select_other_jvm(self):
        self.log()
        self.log(SAMPLE, pid=999)
        corroborated_digest = self.selection_digest()
        self.selection_path.unlink()
        self.write_selection(dict(self.witness, pid=999))
        with mock.patch.object(collector, 'read_checked', wraps=collector.read_checked) as read:
            report = collector.collect(self.root, self.selection_path, corroborated_digest)
        self.assertEqual(report['issues'], ['selection_binding_mismatch'])
        self.assertEqual(report['events'], [])
        self.assertFalse(report['selection_binding_verified'])
        self.assertEqual(len(read.call_args_list), 1)
        self.assertEqual(read.call_args.args[1], self.selection_path)

    def test_identical_bytes_replacement_before_collection_retains_binding(self):
        self.log()
        digest = self.selection_digest()
        data = self.selection_path.read_bytes()
        self.selection_path.unlink()
        self.selection_path.write_bytes(data)
        report = collector.collect(self.root, self.selection_path, digest)
        self.assertEqual(report['status'], 'complete')
        self.assertTrue(report['selection_binding_verified'])
        self.assertEqual(report['selection_sha256'], digest)

    def test_reserialized_witness_requires_its_own_exact_digest(self):
        self.log()
        digest = self.selection_digest()
        self.selection_path.write_text(json.dumps(self.witness, indent=2), encoding='utf-8')
        report = collector.collect(self.root, self.selection_path, digest)
        self.assertEqual(report['issues'], ['selection_binding_mismatch'])

    def test_witness_digest_is_checked_again_after_collection(self):
        self.log()
        digest = self.selection_digest()
        original = collector.parse_logs

        def replace_witness(files, limits=None):
            result = original(files, limits)
            self.write_selection(dict(self.witness, pid=999))
            return result

        with mock.patch.object(collector, 'parse_logs', side_effect=replace_witness):
            report = collector.collect(self.root, self.selection_path, digest)
        self.assertEqual(report['issues'], ['selection_binding_mismatch'])
        self.assertEqual(report['events'], [])
        self.assertEqual(report['files'], [])
        self.assertFalse(report['selection_binding_verified'])

    def test_cli_digest_binding(self):
        self.log()
        for digest, returncode in ((self.selection_digest(), 0), ('0' * 64, 1)):
            process = subprocess.run([
                sys.executable, str(Path(collector.__file__)), '--owned-root', str(self.root),
                '--selection', str(self.selection_path), '--expected-selection-sha256', digest,
            ], capture_output=True, text=True)
            self.assertEqual(process.returncode, returncode)
            report = json.loads(process.stdout)
            self.assertEqual(report['selection_binding_verified'], returncode == 0)
            self.assertNotIn(str(self.root), process.stdout + process.stderr)


if __name__ == '__main__':
    unittest.main()
