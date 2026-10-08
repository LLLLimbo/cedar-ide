#!/usr/bin/env python3
"""Synthetic identity, accounting, privacy and native observation checks."""
import ctypes
import json
import os
from pathlib import Path
import queue
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from unittest import mock
from types import SimpleNamespace

import measure_process_tree as baseline


class FakeClock:
    def __init__(self, now=0):
        self.now = now

    def __call__(self):
        return self.now

    def advance(self, duration):
        self.now += duration


class FakeBackend:
    handle_metric = 'windows_process_handle_count'
    cpu_counter_unit_ns = 100

    def __init__(self):
        self.clock = FakeClock()
        self.entries = {100: (1, 2, 10), 101: (100, 3, 11), 102: (101, 4, 12)}
        self.images = {100: '/PRIVATE/driver', 101: '/PRIVATE/agent', 102: '/PRIVATE/java'}
        self.values = {pid: {'rss_bytes': pid, 'cpu_ns': 10_000, 'threads': threads,
                             'handles': 7} for pid, (_, threads, _) in self.entries.items()}
        self.errors = {}
        self.denied = set()
        self.closed = []
        self.before_read = {}
        self.read_span = {}
        self.after_read = {}
        self.cpu_rates = {}

    def scan(self):
        return self.entries.copy()

    def open(self, pid):
        if pid in self.denied:
            raise baseline.ObservationError('descendant_unavailable')
        return baseline.Process(pid, self.entries[pid][2], baseline.path_key(self.images[pid]))

    def owns_child(self, parent, child):
        return self.entries.get(parent.pid, (0, 0, 0))[2] == parent.created <= child.created

    def read(self, process, scan):
        if process.pid in self.errors:
            raise baseline.ObservationError(self.errors[process.pid])
        if self.entries[process.pid][2] != process.created:
            raise baseline.ObservationError('identity_changed')
        pid = process.pid
        self.clock.advance(self.before_read.get(pid, 0))
        start = self.clock()
        span = self.read_span.get(pid, 0)
        self.clock.advance(span // 2)
        values = self.values[pid].copy()
        if pid in self.cpu_rates:
            values['cpu_ns'] = round(self.clock() * self.cpu_rates[pid])
        self.clock.advance(span - span // 2)
        values.update(cpu_read_start_ns=start, cpu_read_end_ns=self.clock())
        self.clock.advance(self.after_read.get(pid, 0))
        return values

    def close(self, process):
        self.closed.append((process.pid, process.created))


class SamplerTests(unittest.TestCase):
    def setUp(self):
        self.backend = FakeBackend()
        self.sampler = baseline.Sampler(self.backend, 100, {
            'headless_driver': '/PRIVATE/driver', 'agent': '/PRIVATE/agent', 'jvm': '/PRIVATE/java'},
            resolution_ns=1)
        self.addCleanup(self.sampler.close)

    def sample(self, now, phase='starting'):
        self.backend.clock.now = now
        return self.sampler.sample(phase)

    def test_same_sweep_sum_and_cpu_phase_boundaries(self):
        first = self.sample(1_000_000, 'starting')
        self.assertEqual(first['aggregate']['rss_bytes'], 303)
        self.assertEqual(first['aggregate']['threads'], 9)
        self.assertIsNone(first['aggregate'][baseline.CPU_SUM])
        self.assertFalse(first['complete'])
        for values in self.backend.values.values():
            values['cpu_ns'] += 500_000
        second = self.sample(2_000_000, 'starting')
        self.assertEqual(second['aggregate'][baseline.CPU_SUM], 150)
        self.assertTrue(second['complete'])
        transitioned = self.sample(3_000_000, 'semantic_ready_idle')
        self.assertIsNone(transitioned['aggregate'][baseline.CPU_SUM])
        self.assertFalse(transitioned['metric_complete'][baseline.CPU_SUM])

    def test_aggregate_peak_is_never_sum_of_independent_peaks(self):
        self.backend.values[100]['rss_bytes'] = 900
        first = self.sample(1_000_000, 'starting')
        self.backend.values[100]['rss_bytes'] = 100
        self.backend.values[102]['rss_bytes'] = 800
        second = self.sample(2_000_000, 'starting')
        report = baseline.summary([first, second])
        self.assertEqual(report['starting']['rss_bytes']['observed_max'], 1103)
        self.assertNotEqual(report['starting']['rss_bytes']['observed_max'], 1801)

    def test_phase_change_during_sweep_invalidates_both_cpu_endpoints(self):
        self.sample(1_000_000, 'semantic_ready_idle')
        row = self.sample(2_000_000, 'semantic_ready_idle')
        self.sampler.validate_phase(row, 'query_workload', [])
        self.assertFalse(row['phase_stable'])
        self.assertIsNone(row['aggregate'][baseline.CPU_SUM])
        self.assertIsNone(row['by_role']['jvm'][baseline.CPU_SUM])
        next_row = self.sample(3_000_000, 'query_workload')
        self.assertIsNone(next_row['aggregate'][baseline.CPU_SUM])
        self.assertEqual(baseline.summary([row])['semantic_ready_idle']['samples'], 0)

    def test_missing_role_has_no_zero_observation(self):
        del self.backend.entries[102]
        row = self.sample(1_000_000, 'starting')
        self.assertFalse(row['required_roles_observed'])
        self.assertFalse(row['complete'])
        self.assertIsNone(row['by_role']['jvm']['rss_bytes'])
        self.assertIsNone(row['by_role']['jvm']['threads'])
        self.assertIsNone(row['aggregate']['rss_bytes'])
        self.assertIsNone(row['aggregate'][baseline.CPU_SUM])
        self.assertEqual(baseline.summary([row])['starting']['rss_bytes']['valid_samples'], 0)

    def test_denied_and_missing_metrics_stay_unknown(self):
        self.backend.denied.add(102)
        row = self.sample(1_000_000, 'starting')
        self.assertFalse(row['complete'])
        self.assertIsNone(row['aggregate']['rss_bytes'])
        self.assertIn('descendant_unavailable', row['issues'])
        self.backend.denied.clear()
        self.backend.values[102]['rss_bytes'] = None
        row = self.sample(2_000_000, 'starting')
        self.assertIsNone(row['aggregate']['rss_bytes'])
        self.assertIsNone(row['processes'][-1]['rss_bytes'])

    def test_known_children_remain_after_reparenting(self):
        self.sample(1_000_000, 'starting')
        self.backend.entries[102] = (1, 4, 12)
        row = self.sample(2_000_000, 'starting')
        self.assertEqual(len(row['processes']), 3)
        self.assertEqual(row['by_role']['jvm']['rss_bytes'], 102)

    def test_reused_parent_pid_does_not_adopt_unrelated_child(self):
        self.sample(1_000_000, 'starting')
        self.backend.entries[101] = (1, 3, 1000)
        self.backend.entries[103] = (101, 1, 1001)
        self.backend.images[103] = '/PRIVATE/unrelated'
        self.backend.values[103] = self.backend.values[100].copy()
        row = self.sample(2_000_000, 'starting')
        self.assertEqual(len(self.sampler.processes), 3)
        self.assertIsNone(row['aggregate']['rss_bytes'])
        self.assertIn('identity_changed', row['issues'])

    def test_unknown_executable_is_other_descendant(self):
        self.backend.images[102] = '/PRIVATE/java --SECRET token'
        row = self.sample(1_000_000, 'starting')
        self.assertEqual(row['processes'][-1]['role'], 'other_descendant')
        public = json.dumps(row)
        for secret in ('PRIVATE', 'SECRET', 'token', 'pid', 'image', 'created'):
            self.assertNotIn(secret, public)

    def test_hostile_error_text_is_not_serialized(self):
        self.backend.errors[102] = 'SECRET_PATH SECRET_ENV SECRET_STACK'
        row = self.sample(1_000_000, 'starting')
        self.assertEqual(row['issues'], ['observation_failed'])
        self.assertNotIn('SECRET', json.dumps(row))
        self.assertIsNone(row['aggregate']['rss_bytes'])

    def test_unverified_driver_and_unrecognized_role_are_rejected(self):
        with self.assertRaises(baseline.ObservationError):
            baseline.Sampler(self.backend, 100, {'headless_driver': '/wrong'})
        with self.assertRaises(baseline.ObservationError):
            baseline.Sampler(self.backend, 100, {'SECRET_ROLE': '/PRIVATE/driver'})

    def test_process_limit_is_explicit(self):
        with mock.patch.object(baseline, 'MAX_PROCESSES', 2):
            row = self.sample(1_000_000, 'starting')
        self.assertIn('process_limit', row['issues'])
        self.assertIsNone(row['aggregate']['rss_bytes'])

    def test_discovery_failure_never_becomes_an_empty_zero_tree(self):
        with mock.patch.object(self.backend, 'scan', side_effect=baseline.ObservationError('discovery_failed')):
            row = self.sample(1_000_000, 'starting')
        self.assertIsNone(row['aggregate']['rss_bytes'])
        self.assertFalse(row['complete'])

    def test_delayed_process_read_uses_its_own_window(self):
        self.backend.cpu_rates = {100: 0, 101: 0, 102: 4}
        self.sample(1_000_000_000)
        # The sampler is descheduled between reading the agent and JVM.
        self.backend.before_read[102] = 80_000_000
        row = self.sample(1_200_000_000)
        jvm = row['processes'][-1]
        self.assertEqual(jvm['cpu_percent_one_core'], 400)
        self.assertEqual(jvm['cpu_interval']['elapsed_ns'], 280_000_000)
        self.assertEqual(row['processes'][0]['cpu_interval']['elapsed_ns'], 200_000_000)
        self.assertEqual(row['aggregate'][baseline.CPU_SUM], 400)
        self.assertNotIn('cpu_percent_one_core', row['aggregate'])
        self.assertNotIn('cpu_percent_one_core', row['by_role']['jvm'])
        report = baseline.summary([row])['starting']
        self.assertNotIn('cpu_percent_one_core', report)
        self.assertEqual(report[baseline.CPU_SUM]['observed_max'], 400)
        # Old sweep-start arithmetic would have invented 560% on four cores.
        self.assertEqual(100 * jvm['cpu_interval']['cpu_delta_ns'] / 200_000_000, 560)

    def test_previous_slow_sweep_does_not_understate_next_process_rate(self):
        self.backend.cpu_rates[102] = 4
        self.backend.before_read[102] = 80_000_000
        self.sample(1_000_000_000)
        self.backend.before_read.clear()
        row = self.sample(1_200_000_000)
        self.assertEqual(row['processes'][-1]['cpu_percent_one_core'], 400)
        self.assertEqual(row['processes'][-1]['cpu_interval']['elapsed_ns'], 120_000_000)

    def test_scheduler_pause_between_sweeps_counts_as_elapsed_time(self):
        self.sample(1_000_000_000)
        self.backend.values[102]['cpu_ns'] += 500_000_000
        row = self.sample(3_000_000_000)
        self.assertEqual(row['processes'][-1]['cpu_percent_one_core'], 25)
        self.assertEqual(row['processes'][-1]['cpu_interval']['elapsed_ns'], 2_000_000_000)

    def test_slow_counter_calls_expose_read_placement_uncertainty(self):
        self.backend.cpu_rates[102] = 4
        self.backend.read_span[102] = 2_000_000
        self.sample(1_000_000_000)
        self.backend.read_span[102] = 80_000_000
        row = self.sample(1_200_000_000)
        jvm = row['processes'][-1]
        interval = jvm['cpu_interval']
        self.assertEqual(jvm['cpu_percent_one_core'], 400)
        self.assertEqual(interval['status'], 'estimated')
        self.assertEqual(interval['previous_read_span_ns'], 2_000_000)
        self.assertEqual(interval['current_read_span_ns'], 80_000_000)
        self.assertEqual(interval['elapsed_ns'], 239_000_000)
        self.assertEqual(interval['elapsed_min_ns'], 198_000_000 - 2)
        self.assertEqual(interval['elapsed_max_ns'], 280_000_000 + 2)
        self.assertLess(interval['percent_lower_from_timing'], 400)
        self.assertGreater(interval['percent_upper_from_timing'], 400)

    def test_non_cpu_api_delay_does_not_widen_that_process_counter_bracket(self):
        self.backend.cpu_rates = {100: 1, 101: 0, 102: 0}
        self.sample(1_000_000_000)
        self.backend.after_read[100] = 80_000_000
        row = self.sample(1_200_000_000)
        driver = row['processes'][0]
        self.assertEqual(driver['cpu_percent_one_core'], 100)
        self.assertEqual(driver['cpu_interval']['current_read_span_ns'], 0)
        self.assertEqual(driver['cpu_interval']['elapsed_ns'], 200_000_000)
        self.assertEqual(row['processes'][-1]['cpu_interval']['elapsed_ns'], 280_000_000)

    def test_cpu_is_not_clipped_to_machine_capacity(self):
        self.backend.cpu_rates[102] = 8
        with mock.patch.object(baseline.os, 'cpu_count', return_value=4):
            self.sample(1_000_000_000)
            row = self.sample(1_200_000_000)
        # This is a deliberately implausible fake counter, not a real measurement.
        self.assertEqual(row['processes'][-1]['cpu_percent_one_core'], 800)
        self.assertEqual(row['aggregate'][baseline.CPU_SUM], 800)

    def test_read_phase_change_discards_estimate_and_timing_bounds(self):
        self.backend.cpu_rates[102] = 4
        self.sample(1_000_000_000, 'semantic_ready_idle')
        self.backend.read_span[102] = 80_000_000
        row = self.sample(1_200_000_000, 'semantic_ready_idle')
        self.sampler.validate_phase(row, 'query_workload', [])
        interval = row['processes'][-1]['cpu_interval']
        self.assertEqual(interval['status'], 'phase_unstable')
        self.assertTrue(all(interval[key] is None for key in baseline.CPU_INTERVAL_FIELDS))
        self.assertIsNone(row['processes'][-1]['cpu_percent_one_core'])
        self.assertEqual(row['aggregate']['rss_bytes'], 303)
        next_row = self.sample(1_400_000_000, 'query_workload')
        self.assertIsNone(next_row['processes'][-1]['cpu_percent_one_core'])
        self.assertEqual(next_row['processes'][-1]['cpu_interval']['status'], 'phase_boundary')

    def test_invalid_cpu_timing_preserves_identity_and_live_resources(self):
        self.sample(1_000_000_000)
        original = self.backend.read

        def broken_clock(process, scan):
            values = original(process, scan)
            if process.role == 'jvm':
                values['cpu_read_end_ns'] = values['cpu_read_start_ns'] - 1
            return values

        with mock.patch.object(self.backend, 'read', side_effect=broken_clock):
            row = self.sample(1_200_000_000)
        self.assertTrue(row['processes'][-1]['identity_verified'])
        self.assertEqual(row['aggregate']['rss_bytes'], 303)
        self.assertEqual(row['aggregate']['threads'], 9)
        self.assertTrue(row['metric_complete']['rss_bytes'])
        self.assertFalse(row['metric_complete'][baseline.CPU_SUM])
        self.assertEqual(row['issues'], ['cpu_timing_invalid'])
        self.assertFalse(row['complete'])
        self.assertIsNone(self.sample(1_400_000_000)['aggregate'][baseline.CPU_SUM])
        self.assertIsNotNone(self.sample(1_600_000_000)['aggregate'][baseline.CPU_SUM])


class CpuIntervalTests(unittest.TestCase):
    def reading(self, process, cpu, start, end, resolution=1):
        return baseline.cpu_interval(process, {'cpu_ns': cpu, 'cpu_read_start_ns': start,
                                               'cpu_read_end_ns': end}, 'starting', resolution)

    def test_coarse_quantization_never_becomes_a_precise_short_interval(self):
        process = baseline.Process(100, 10, '/PRIVATE/driver')
        self.reading(process, 0, 0, 0, resolution=16_000_000)
        percent, interval, issue = self.reading(process, 20_000_000, 16_000_000,
                                               16_000_000, resolution=16_000_000)
        self.assertIsNone(percent)
        self.assertEqual(interval['status'], 'interval_unresolved')
        self.assertEqual(issue, 'cpu_timing_invalid')
        self.assertEqual(interval['elapsed_min_ns'], -16_000_000)
        self.assertIsNone(process.previous_read)

    def test_long_coarse_clock_interval_exposes_quantization(self):
        process = baseline.Process(100, 10, '/PRIVATE/driver')
        self.reading(process, 0, 0, 0, resolution=16_000_000)
        percent, interval, issue = self.reading(process, 200_000_000, 200_000_000,
                                               200_000_000, resolution=16_000_000)
        self.assertEqual(percent, 100)
        self.assertIsNone(issue)
        self.assertEqual(interval['elapsed_min_ns'], 168_000_000)
        self.assertEqual(interval['elapsed_max_ns'], 232_000_000)

    def test_equal_reversed_overlapping_and_adjacent_read_windows_are_unknown(self):
        for start, end in ((100, 110), (90, 95), (105, 120), (110, 120), (120, 119)):
            with self.subTest(start=start, end=end):
                process = baseline.Process(100, 10, '/PRIVATE/driver')
                self.reading(process, 10, 100, 110)
                percent, interval, issue = self.reading(process, 20, start, end)
                self.assertIsNone(percent)
                self.assertIn(interval['status'], ('interval_unresolved', 'clock_invalid'))
                self.assertEqual(issue, 'cpu_timing_invalid')
                self.assertIsNone(process.previous_cpu)
                # The next good observation only reestablishes a baseline.
                self.assertIsNone(self.reading(process, 30, 200, 210)[0])
                self.assertIsNotNone(self.reading(process, 40, 300, 310)[0])

    def test_invalid_timing_types_and_cpu_counter_are_fixed_statuses(self):
        cases = ((20, 'SECRET', 120, 1, 'clock_invalid'),
                 (20, 100, float('nan'), 1, 'clock_invalid'),
                 (20, True, 120, 1, 'clock_invalid'),
                 (20, 100, 120, 0, 'clock_invalid'),
                 (20, 100, 120, 1.5, 'clock_invalid'),
                 ('SECRET', 100, 120, 1, 'counter_invalid'),
                 (None, 100, 120, 1, 'counter_invalid'),
                 (True, 100, 120, 1, 'counter_invalid'),
                 (-1, 100, 120, 1, 'counter_invalid'),
                 (5, 100, 120, 1, 'counter_regressed'))
        for cpu, start, end, resolution, status in cases:
            with self.subTest(status=status):
                process = baseline.Process(100, 10, '/PRIVATE/driver')
                self.reading(process, 10, 0, 10)
                result = self.reading(process, cpu, start, end, resolution)
                self.assertIsNone(result[0])
                self.assertEqual(result[1]['status'], status)
                self.assertNotIn('SECRET', json.dumps(result, allow_nan=False))

    def test_fractional_rate_stays_inside_unrounded_bounds(self):
        process = baseline.Process(100, 10, '/PRIVATE/driver')
        self.reading(process, 0, 0, 0)
        percent, interval, issue = self.reading(process, 12_345_678, 1_000_000_003, 1_000_000_003)
        self.assertIsNone(issue)
        self.assertLessEqual(interval['percent_lower_from_timing'], percent)
        self.assertGreaterEqual(interval['percent_upper_from_timing'], percent)


class BackendTimingTests(unittest.TestCase):
    def test_windows_brackets_get_process_times_without_later_memory_handle_waits(self):
        class FileTime(ctypes.Structure):
            _fields_ = [('dwLowDateTime', ctypes.c_uint32), ('dwHighDateTime', ctypes.c_uint32)]

        class Memory(ctypes.Structure):
            _fields_ = [('size', ctypes.c_uint32), ('rss', ctypes.c_size_t)]

        clock = FakeClock(1_000_000_000)
        backend = baseline.Windows.__new__(baseline.Windows)
        backend.clock = clock
        backend.w = SimpleNamespace(FILETIME=FileTime, DWORD=ctypes.c_uint32)
        backend.Memory = Memory

        def get_times(handle, *values):
            clock.advance(8_000_000)
            for pointer, value in zip(values, (10, 0, 123, 456)):
                pointer._obj.dwLowDateTime = value
            return True

        def get_memory(handle, memory, size):
            clock.advance(50_000_000)
            memory._obj.rss = 8192
            return True

        def get_handles(handle, count):
            clock.advance(40_000_000)
            count._obj.value = 7
            return True

        def wait(handle, timeout):
            clock.advance(5_000_000)
            return 258

        backend.k = SimpleNamespace(GetProcessTimes=get_times, GetProcessHandleCount=get_handles,
                                    WaitForSingleObject=wait)
        backend.p = SimpleNamespace(GetProcessMemoryInfo=get_memory)
        process = baseline.Process(100, 10, '/PRIVATE/driver', native=123)
        result = backend.read(process, {100: (1, 2, None)})
        self.assertEqual(result['cpu_read_start_ns'], 1_000_000_000)
        self.assertEqual(result['cpu_read_end_ns'], 1_008_000_000)
        self.assertEqual(clock(), 1_108_000_000)
        self.assertEqual(result['cpu_ns'], 57_900)
        self.assertEqual(result['rss_bytes'], 8192)
        self.assertEqual(result['handles'], 7)

    def test_linux_brackets_the_stat_that_supplies_cpu_without_fd_scan(self):
        clock = FakeClock(1_000_000_000)
        backend = baseline.Linux.__new__(baseline.Linux)
        backend.clock, backend.tick_ns, backend.page_bytes = clock, 10_000_000, 4096
        reads = []

        def stat(pid):
            reads.append(pid)
            clock.advance(3_000_000 if len(reads) == 1 else 8_000_000)
            return {'created': 10, 'cpu': 123 if len(reads) == 1 else 456,
                    'zombie': False, 'threads': 2, 'rss': 2}

        def descriptors():
            clock.advance(50_000_000)
            return iter(range(7))

        process = baseline.Process(100, 10, '/PRIVATE/driver')
        with mock.patch.object(backend, 'stat', side_effect=stat), \
                mock.patch.object(baseline.Path, 'iterdir', side_effect=descriptors):
            result = backend.read(process, {})
        self.assertEqual(result['cpu_read_start_ns'], 1_053_000_000)
        self.assertEqual(result['cpu_read_end_ns'], 1_061_000_000)
        self.assertEqual(result['cpu_ns'], 4_560_000_000)
        self.assertEqual(result['rss_bytes'], 8192)
        self.assertEqual(result['handles'], 7)


class MarkerTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='cedar-resource-markers-')
        self.addCleanup(self.temporary.cleanup)
        self.path = Path(self.temporary.name, 'markers.jsonl')

    def test_fixed_fields_only_and_ordered_readiness(self):
        self.path.write_text(''.join(json.dumps({'phase': phase, 'elapsed_ms': index * 10}) + '\n'
                                    for index, phase in enumerate(baseline.PHASES)))
        events, issues = baseline.phase_events(self.path)
        self.assertFalse(issues)
        self.assertEqual([event['phase'] for event in events], list(baseline.PHASES))

    def test_hostile_marker_is_rejected_not_merged(self):
        for marker in ({'phase': 'SECRET_ROLE', 'elapsed_ms': 0},
                       {'phase': 'starting', 'elapsed_ms': 0, 'environment': 'SECRET_ENV'},
                       {'phase': 'starting', 'elapsed_ms': 'SECRET_PATH'},
                       {'phase': 'semantic_ready_idle', 'elapsed_ms': 0},
                       {'phase': 'starting', 'elapsed_ms': True}):
            with self.subTest(marker=marker):
                self.path.write_text(json.dumps(marker) + '\n')
                result = baseline.phase_events(self.path)
                self.assertEqual(result, ([], ['phase_invalid']))
                self.assertNotIn('SECRET', json.dumps(result))

    def test_partial_write_is_retried_and_size_is_bounded(self):
        self.path.write_bytes(b'{"phase":"starting","elapsed_ms":0}\n{"phase":')
        self.assertEqual(len(baseline.phase_events(self.path)[0]), 1)
        self.path.write_bytes(b'X' * (baseline.MAX_MARKER_BYTES + 1))
        self.assertEqual(baseline.phase_events(self.path), ([], ['phase_limit']))


class RunFailureTests(unittest.TestCase):
    def test_run_uses_perf_counter_for_elapsed_sweep_and_pacing_and_preserves_exit(self):
        with tempfile.TemporaryDirectory(prefix='cedar-resource-clock-') as root:
            args = SimpleNamespace(driver='/PRIVATE/driver', agent='/PRIVATE/agent',
                                   java='/PRIVATE/java', source_commit='a' * 40,
                                   interval_ms=200, timeout_seconds=270,
                                   phase_file=str(Path(root, 'phases')),
                                   transcript=str(Path(root, 'private')),
                                   output=str(Path(root, 'report.json')))
            backend = FakeBackend()
            backend.clock.now = 1_000_000_000
            backend.read_span = {pid: 1_000_000 for pid in backend.entries}
            child = mock.Mock(pid=100, returncode=42)
            child.poll.side_effect = (None, 42)
            factory = 'Windows' if sys.platform == 'win32' else 'Linux'
            with mock.patch.object(baseline, factory, return_value=backend), \
                    mock.patch.object(baseline.subprocess, 'Popen', return_value=child), \
                    mock.patch.object(baseline.time, 'perf_counter_ns', backend.clock), \
                    mock.patch.object(baseline.time, 'monotonic_ns', side_effect=AssertionError('coarse clock')), \
                    mock.patch.object(baseline.time, 'sleep',
                                      side_effect=lambda seconds: backend.clock.advance(round(seconds * 1e9))):
                self.assertEqual(baseline.run(args), 42)
            report = json.loads(Path(args.output).read_text())
            self.assertEqual(report['schema_version'], 2)
            self.assertEqual(report['driver_exit_code'], 42)
            self.assertEqual(report['measurement']['clock'], 'time.perf_counter_ns')
            self.assertEqual(report['measurement']['cpu_counter_unit_ns'], 100)
            self.assertEqual([row['elapsed_ms'] for row in report['samples']], [0, 200])
            self.assertEqual([row['sweep_ms'] for row in report['samples']], [3, 3])
            self.assertEqual(report['samples'][1]['processes'][0]['cpu_interval']['elapsed_ns'],
                             200_000_000)
            public = json.dumps(report, allow_nan=False)
            # Read-span durations are allowed; absolute QPC endpoints are not.
            for secret in ('PRIVATE', 'cpu_read_start_ns', 'cpu_read_end_ns',
                           '"previous_read"', '1000000000'):
                self.assertNotIn(secret, public)

    def test_already_exited_driver_code_survives_observer_failure(self):
        with tempfile.TemporaryDirectory(prefix='cedar-resource-failure-') as root:
            args = SimpleNamespace(driver='/PRIVATE/driver', agent='/PRIVATE/agent',
                                   java='/PRIVATE/java', source_commit='a' * 40,
                                   interval_ms=200, timeout_seconds=270,
                                   phase_file=str(Path(root, 'phases')),
                                   transcript=str(Path(root, 'private')),
                                   output=str(Path(root, 'report.json')))
            child = mock.Mock(returncode=42)
            child.poll.return_value = 42
            backend = 'Windows' if sys.platform == 'win32' else 'Linux'
            with mock.patch.object(baseline, backend) as factory, \
                    mock.patch.object(baseline.subprocess, 'Popen', return_value=child), \
                    mock.patch.object(baseline, 'Sampler', side_effect=OSError('SECRET_PATH')):
                factory.return_value.handle_metric = 'windows_process_handle_count'
                factory.return_value.cpu_counter_unit_ns = 100
                self.assertEqual(baseline.run(args), 42)
            report = Path(args.output).read_text()
            self.assertEqual(json.loads(report)['driver_exit_code'], 42)
            for secret in ('SECRET', 'PRIVATE', '/driver', '/agent', '/java'):
                self.assertNotIn(secret, report)

    @unittest.skipUnless(sys.platform == 'linux', 'Linux missing final identity behavior')
    def test_missing_proc_identity_does_not_become_verified_exit(self):
        backend = baseline.Linux()
        process = baseline.Process(999999, 10, '/PRIVATE/driver')
        with mock.patch.object(backend, 'stat', side_effect=FileNotFoundError('SECRET')):
            with self.assertRaises(baseline.ObservationError):
                backend.read(process, {})
        self.assertFalse(process.exited)


@unittest.skipUnless(sys.platform in ('win32', 'linux'), 'native observer backend required')
class NativeObservationTests(unittest.TestCase):
    def test_real_owned_python_child_tree(self):
        # One generated Python tree, no Java or product launch. The helper owns
        # and always joins its child, even if the observer assertions fail.
        code = ('import subprocess,sys; '
                'child=subprocess.Popen([sys.executable,"-c","import time; time.sleep(12)"]); '
                'print("ready",flush=True); sys.stdin.readline(); child.terminate(); child.wait()')
        root = subprocess.Popen([sys.executable, '-c', code], stdin=subprocess.PIPE,
                                stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
        ready = queue.Queue(maxsize=1)
        reader = threading.Thread(target=lambda: ready.put(root.stdout.readline()), daemon=True)
        reader.start()
        sampler = None
        try:
            try:
                self.assertEqual(ready.get(timeout=3).strip(), 'ready')
            except queue.Empty:
                self.fail('Generated process-tree helper did not become ready within three seconds')
            backend = baseline.Windows() if sys.platform == 'win32' else baseline.Linux()
            sampler = baseline.Sampler(backend, root.pid, {'headless_driver': sys.executable})
            first = sampler.sample('starting')
            time.sleep(0.05)
            second = sampler.sample('starting')
            self.assertGreaterEqual(len(second['processes']), 2)
            self.assertIsNotNone(second['aggregate']['rss_bytes'])
            self.assertGreater(second['aggregate']['rss_bytes'], 0)
            self.assertGreater(second['aggregate']['threads'], 0)
            self.assertGreater(second['aggregate']['handles'], 0)
            self.assertIsNotNone(second['aggregate'][baseline.CPU_SUM])
            self.assertIsNone(first['aggregate'][baseline.CPU_SUM])
        finally:
            if sampler is not None:
                sampler.close()
            # EOF asks the helper to terminate and join its exact child.
            root.stdin.close()
            try:
                root.wait(timeout=5)
            except subprocess.TimeoutExpired:
                root.kill()
                root.wait(timeout=3)
                self.fail('Generated helper did not join its child within the cleanup deadline')
            finally:
                reader.join(timeout=1)
                root.stdout.close()
            self.assertEqual(root.returncode, 0)


if __name__ == '__main__':
    unittest.main()
