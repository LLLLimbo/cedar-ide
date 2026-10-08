#!/usr/bin/env python3
"""Synthetic identity, accounting, privacy and native observation checks."""
import ctypes
import copy
import hashlib
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


class LongMarkerTests(MarkerTests):
    @staticmethod
    def markers():
        events = []
        for phase in baseline.LONG_PHASES:
            events.append({'phase': phase, 'elapsed_ms': len(events) * 10})
            for latency, during in zip(baseline.LATENCIES, baseline.LATENCY_PHASES):
                if during == phase:
                    events.append({'latency': latency, 'elapsed_ms': len(events) * 10,
                                   'duration_ns': 123_456_789})
        return events

    def test_fixed_latency_sequence_and_nanosecond_precision(self):
        self.path.write_text(''.join(json.dumps(event) + '\n' for event in self.markers()))
        phases, latencies, issues = baseline.marker_events(self.path, 'long_idle_baseline')
        self.assertFalse(issues)
        self.assertEqual([event['phase'] for event in phases], list(baseline.LONG_PHASES))
        self.assertEqual([event['latency'] for event in latencies], list(baseline.LATENCIES))
        self.assertEqual(latencies[0]['duration_ns'], 123_456_789)
        self.assertEqual(baseline.phase_events(self.path), ([], ['phase_invalid']))

    def test_unknown_fields_types_bounds_duplicates_and_order_are_rejected(self):
        mutations = []
        for value in ('SECRET_TEXT', True, 1.25, -1, 300_000_000_001):
            item = self.markers()
            item[1]['duration_ns'] = value
            mutations.append(item)
        for field, value in (('latency', 'SECRET_OPERATION'), ('environment', 'SECRET_ENV'),
                             ('elapsed_ms', -1)):
            item = self.markers()
            item[1][field] = value
            mutations.append(item)
        item = self.markers()
        item[1], item[3] = item[3], item[1]
        mutations.append(item)
        item = self.markers()
        item.insert(2, item[1])
        mutations.append(item)
        item = self.markers()
        item[1], item[2] = item[2], item[1]
        item[1]['elapsed_ms'], item[2]['elapsed_ms'] = 10, 20
        mutations.append(item)
        for events in mutations:
            self.path.write_text(''.join(json.dumps(event) + '\n' for event in events))
            result = baseline.marker_events(self.path, 'long_idle_baseline')
            self.assertEqual(result, ([], [], ['phase_invalid']))
            self.assertNotIn('SECRET', json.dumps(result))
        self.path.write_text('{"phase":"SECRET","phase":"starting","elapsed_ms":0}\n')
        self.assertEqual(baseline.phase_events(self.path), ([], ['phase_invalid']))


class LongWindowTests(unittest.TestCase):
    @staticmethod
    def events(duration=30_000):
        return [{'phase': 'semantic_ready_idle', 'elapsed_ms': 1234},
                {'phase': 'query_workload', 'elapsed_ms': 1234 + duration},
                {'phase': 'correction_ready_idle', 'elapsed_ms': 42_000},
                {'phase': 'closing', 'elapsed_ms': 42_000 + duration}]

    @staticmethod
    def rows(phase='semantic_ready_idle', duration=30_000, offset=100_000):
        backend = FakeBackend()
        backend.cpu_rates = {100: 0, 101: 0.01, 102: 0.1}
        backend.read_span = {pid: 1_000_000 for pid in backend.entries}
        sampler = baseline.Sampler(backend, 100, {
            'headless_driver': '/PRIVATE/driver', 'agent': '/PRIVATE/agent', 'jvm': '/PRIVATE/java'},
            resolution_ns=1, origin_ns=0)
        rows = []
        for elapsed in range(offset, offset + duration + 201, 200):
            backend.clock.now = elapsed * 1_000_000
            observed_phase = phase if elapsed <= offset + duration else baseline.LONG_PHASES[baseline.LONG_PHASES.index(phase) + 1]
            row = sampler.sample(observed_phase)
            row.update(sweep_start_elapsed_ns=elapsed * 1_000_000,
                       sweep_end_elapsed_ns=backend.clock.now)
            rows.append(row)
        sampler.close()
        return rows

    def window(self, rows, events=None):
        return baseline.idle_windows(rows, self.events() if events is None else events, 200)['semantic_ready_idle']

    def test_final_ten_seconds_use_sampler_domain_and_actual_coverage(self):
        window = self.window(self.rows())
        self.assertEqual(window['status'], 'observed')
        self.assertEqual(window['observed_marker_duration_ms'], 30_000)
        self.assertEqual(window['observed_end_elapsed_ns'], 130_003_000_000)
        self.assertEqual(window['requested_start_elapsed_ns'], 120_003_000_000)
        self.assertEqual(window['first_sample_elapsed_ns'], 120_200_000_000)
        self.assertEqual(window['observed_sample_span_ns'], 9_803_000_000)
        self.assertEqual(window['stable_samples'], 50)
        self.assertEqual(window['maximum_unsampled_gap_ns'], 197_000_000)
        self.assertEqual(window['trailing_phase_observation_gap_ns'], 200_000_000)
        self.assertEqual(window['rss_bytes']['tree']['median'], 303)
        for item in window['cpu']['processes']:
            self.assertEqual(item['status'], 'observed')
            self.assertEqual(item['interval_count'], 49)
            self.assertGreaterEqual(item['first_read_elapsed_ns'], window['requested_start_elapsed_ns'])
            self.assertLessEqual(item['last_read_elapsed_ns'], window['observed_end_elapsed_ns'])
        self.assertEqual(window['cpu']['tree']['sum_of_independent_process_estimates_percent_one_core'], 11)
        self.assertEqual(window['cpu']['by_role']['jvm']['sum_of_independent_process_estimates_percent_one_core'], 10)

    def test_unstable_sweeps_inside_and_outside_window(self):
        rows = self.rows()
        rows[-1]['phase'] = 'semantic_ready_idle'
        rows[-1]['phase_stable'] = False
        rows[-1]['observed_phase_after_sweep'] = 'query_workload'
        self.assertEqual(self.window(rows)['status'], 'observed')
        rows[-20]['phase_stable'] = False
        window = self.window(rows)
        self.assertEqual(window['status'], 'unknown')
        self.assertEqual(window['unstable_samples'], 1)
        self.assertIsNone(window['rss_bytes']['tree']['median'])

    def test_missing_short_and_unfinished_windows_stay_unknown(self):
        for rows, events in (([], self.events()), (self.rows(duration=5000), self.events()),
                             (self.rows(), []), (self.rows(), self.events(duration=29_999))):
            window = self.window(rows, events)
            self.assertEqual(window['status'], 'unknown')
            self.assertIsNone(window['rss_bytes']['tree']['median'])
            self.assertIsNone(window['cpu']['tree']['sum_of_independent_cpu_deltas_ns'])
        empty = self.window([])
        self.assertIsNone(empty['observed_sample_span_ns'])
        self.assertIsNone(empty['maximum_unsampled_gap_ns'])

    def test_large_sampling_and_trailing_gaps_are_unknown(self):
        rows = self.rows()
        del rows[-20:-17]
        window = self.window(rows)
        self.assertEqual(window['status'], 'unknown')
        self.assertEqual(window['maximum_unsampled_gap_ns'], 797_000_000)
        self.assertIsNone(window['rss_bytes']['tree']['median'])
        rows = self.rows()
        rows[-1]['sweep_start_elapsed_ns'] += 5_000_000_000
        rows[-1]['sweep_end_elapsed_ns'] += 5_000_000_000
        self.assertEqual(self.window(rows)['status'], 'unknown')
        self.assertEqual(self.window(rows)['trailing_phase_observation_gap_ns'], 5_200_000_000)
        self.assertEqual(self.window(self.rows()[:-1])['status'], 'unknown')

    def test_missing_rss_or_cpu_is_not_filled_from_other_samples(self):
        rows = self.rows()
        rows[-20]['aggregate']['rss_bytes'] = None
        rows[-20]['processes'][-1]['cpu_interval'] = baseline.empty_cpu_interval('unavailable')
        window = self.window(rows)
        self.assertEqual(window['status'], 'observed')
        self.assertEqual(window['rss_bytes']['tree']['unknown_samples'], 1)
        self.assertIsNone(window['rss_bytes']['tree']['median'])
        self.assertEqual(window['rss_bytes']['by_role']['jvm']['median'], 102)
        self.assertEqual(window['cpu']['processes'][-1]['discontinuities'], 1)
        self.assertEqual(window['cpu']['processes'][-1]['status'], 'unknown')
        self.assertGreater(window['cpu']['processes'][-1]['observed_cpu_delta_ns'], 0)
        self.assertIsNone(window['cpu']['tree']['sum_of_independent_cpu_deltas_ns'])
        rows = self.rows()
        rows[-2]['processes'][-1]['cpu_interval'] = baseline.empty_cpu_interval('unavailable')
        window = self.window(rows)
        self.assertEqual(window['cpu']['processes'][-1]['unknown_samples'], 1)
        self.assertIsNone(window['cpu']['processes'][-1]['percent_one_core'])

    def test_integrated_counter_evidence_retains_independent_intervals(self):
        rows = self.rows()
        for row in rows[-26:-1]:
            value = row['processes'][-1]['cpu_interval']
            value['cpu_delta_ns'] *= 2
        window = self.window(rows)
        jvm = window['cpu']['processes'][-1]
        self.assertEqual(jvm['observed_cpu_delta_ns'], 1_480_000_000)
        self.assertEqual(jvm['observed_interval_elapsed_ns'], 9_800_000_000)
        self.assertAlmostEqual(jvm['percent_one_core'], 100 * 1.48 / 9.8)
        self.assertLess(jvm['percent_lower_from_timing'], jvm['percent_one_core'])
        self.assertGreater(jvm['percent_upper_from_timing'], jvm['percent_one_core'])
        self.assertNotEqual(window['cpu']['processes'][0]['first_read_elapsed_ns'], jvm['first_read_elapsed_ns'])

    def test_both_idle_phases_are_reported_individually(self):
        rows = self.rows() + self.rows('correction_ready_idle', offset=200_000)
        windows = baseline.idle_windows(rows, self.events(), 200)
        self.assertEqual(set(windows), set(baseline.IDLE_PHASES))
        self.assertTrue(all(window['status'] == 'observed' for window in windows.values()))
        self.assertNotEqual(windows['semantic_ready_idle']['observed_end_elapsed_ns'],
                            windows['correction_ready_idle']['observed_end_elapsed_ns'])

    def test_variable_length_counter_intervals_are_weighted_by_elapsed_time(self):
        rows = self.rows()
        # Merge alternating adjacent intervals into one longer interval. The
        # retained counter increments represent 100% on long intervals, 0% on
        # short ones, so averaging per-sample percentages would be incorrect.
        retained = []
        index = 0
        while index < len(rows) - 1:
            row = copy.deepcopy(rows[index])
            if index > 0 and index % 3 == 2:
                previous = rows[index - 1]['processes'][-1]['cpu_interval']
                current = row['processes'][-1]['cpu_interval']
                for key in ('previous_read_start_elapsed_ns', 'previous_read_end_elapsed_ns'):
                    current[key] = previous[key]
                current['elapsed_ns'] += previous['elapsed_ns']
                current['elapsed_min_ns'] += previous['elapsed_min_ns']
                current['elapsed_max_ns'] += previous['elapsed_max_ns']
                current['cpu_delta_ns'] = current['elapsed_ns']
                row['processes'][-1]['cpu_percent_one_core'] = 100
            elif index > 0:
                row['processes'][-1]['cpu_interval']['cpu_delta_ns'] = 0
                row['processes'][-1]['cpu_percent_one_core'] = 0
            if index % 3 != 1:
                retained.append(row)
            index += 1
        retained.append(rows[-1])
        # Other roles have discontinuities because this fixture merged only
        # the JVM's endpoints. Its own independent integrated estimate remains
        # available even while whole-tree integration is correctly unknown.
        window = self.window(retained)
        jvm = window['cpu']['processes'][-1]
        self.assertEqual(jvm['status'], 'observed')
        self.assertAlmostEqual(jvm['percent_one_core'], 100 * jvm['observed_cpu_delta_ns'] / jvm['observed_interval_elapsed_ns'])
        rates = [row['processes'][-1]['cpu_percent_one_core'] for row in retained
                 if row['phase'] == 'semantic_ready_idle'
                 and row['sweep_start_elapsed_ns'] >= window['first_sample_elapsed_ns']]
        self.assertGreater(abs(jvm['percent_one_core'] - sum(rates) / len(rates)), 10)


class FingerprintBoundsTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix='cedar-fingerprint-bounds-')
        self.addCleanup(temporary.cleanup)
        self.path = Path(temporary.name, 'SECRET-binary')
        self.path.write_bytes(b'input')
        self.args = SimpleNamespace(driver=self.path, agent=self.path, java=self.path)

    def assert_unavailable(self, result):
        fingerprints, issues = result
        self.assertEqual(issues, ['input_fingerprint_unavailable'] * 3)
        self.assertTrue(all(item == {'sha256': None, 'bytes': None} for item in fingerprints.values()))
        self.assertNotIn('SECRET', json.dumps(result))

    def test_native_python_executable_fingerprint_is_valid_and_matching(self):
        executable = Path(sys.executable)
        args = SimpleNamespace(driver=executable, agent=executable, java=executable)
        fingerprints, issues = baseline.input_fingerprints(args)
        self.assertFalse(issues)
        expected = {'sha256': hashlib.sha256(executable.read_bytes()).hexdigest(),
                    'bytes': executable.stat().st_size}
        self.assertEqual(fingerprints, {role: expected for role in baseline.ROLES[:3]})

    def test_windows_path_execute_bits_do_not_invalidate_same_handle_identity(self):
        real_stat, real_fstat = os.stat, os.fstat

        def executable_mode(info, executable):
            copied = {key: getattr(info, key) for key in ('st_dev', 'st_ino', 'st_mode',
                                                         'st_size', 'st_mtime_ns', 'st_ctime_ns')}
            copied['st_mode'] = (copied['st_mode'] & ~0o111) | (0o111 if executable else 0)
            return SimpleNamespace(**copied)

        with mock.patch.object(baseline.os, 'stat', side_effect=lambda path: executable_mode(real_stat(path), True)), \
                mock.patch.object(baseline.os, 'fstat', side_effect=lambda fd: executable_mode(real_fstat(fd), False)):
            fingerprints, issues = baseline.input_fingerprints(self.args)
        self.assertFalse(issues)
        self.assertTrue(all(item['sha256'] == hashlib.sha256(b'input').hexdigest()
                            and item['bytes'] == 5 for item in fingerprints.values()))

    def test_windows_path_birthtime_and_handle_changetime_can_differ_but_must_be_stable(self):
        real_stat = os.stat
        calls = []

        def birthtime_stat(path):
            info = real_stat(path)
            copied = {key: getattr(info, key) for key in ('st_dev', 'st_ino', 'st_mode',
                                                         'st_size', 'st_mtime_ns', 'st_ctime_ns')}
            # Simulate Windows 3.12 path stat exposing a different historical
            # creation time while handle fstat retains its current ChangeTime.
            copied['st_ctime_ns'] -= 1_000_000_000
            return SimpleNamespace(**copied)

        with mock.patch.object(baseline.os, 'stat', side_effect=birthtime_stat):
            fingerprints, issues = baseline.input_fingerprints(self.args)
        self.assertFalse(issues)
        self.assertTrue(all(item['sha256'] == hashlib.sha256(b'input').hexdigest()
                            and item['bytes'] == 5 for item in fingerprints.values()))

        def changed_path_ctime(path):
            info = birthtime_stat(path)
            calls.append(path)
            if len(calls) % 2 == 0:
                info.st_ctime_ns += 1
            return info

        with mock.patch.object(baseline.os, 'stat', side_effect=changed_path_ctime):
            self.assert_unavailable(baseline.input_fingerprints(self.args))

    def test_oversized_inputs_are_rejected_before_open(self):
        with mock.patch.object(baseline, 'MAX_INPUT_FILE_BYTES', 4), \
                mock.patch.object(baseline.os, 'open', side_effect=AssertionError('must not open')):
            self.assert_unavailable(baseline.input_fingerprints(self.args))

    def test_nonregular_inputs_are_rejected_before_open(self):
        self.args.driver = self.args.agent = self.args.java = self.path.parent
        with mock.patch.object(baseline.os, 'open', side_effect=AssertionError('must not open')):
            self.assert_unavailable(baseline.input_fingerprints(self.args))
        if hasattr(os, 'mkfifo'):
            fifo = self.path.parent / 'SECRET-pipe'
            os.mkfifo(fifo)
            self.args.driver = self.args.agent = self.args.java = fifo
            with mock.patch.object(baseline.os, 'open', side_effect=AssertionError('must not open')):
                self.assert_unavailable(baseline.input_fingerprints(self.args))

    def test_mutating_size_identity_or_mtime_invalidates_fingerprint(self):
        real_fstat = os.fstat
        for field in ('st_size', 'st_ino', 'st_dev', 'st_mtime_ns', 'st_ctime_ns'):
            calls = []

            def changing_stat(descriptor):
                info = real_fstat(descriptor)
                calls.append(descriptor)
                if len(calls) % 2:
                    return info
                copied = {key: getattr(info, key) for key in ('st_dev', 'st_ino', 'st_mode',
                                                             'st_size', 'st_mtime_ns', 'st_ctime_ns')}
                copied[field] += 1
                return SimpleNamespace(**copied)

            with self.subTest(field=field), mock.patch.object(baseline.os, 'fstat', side_effect=changing_stat):
                self.assert_unavailable(baseline.input_fingerprints(self.args))

    def test_growing_input_read_is_capped_at_initial_size_plus_one(self):
        reads = []

        class GrowingSource:
            def __init__(self, descriptor, mode):
                self.descriptor = descriptor

            def __enter__(self):
                return self

            def __exit__(self, *arguments):
                os.close(self.descriptor)

            def fileno(self):
                return self.descriptor

            def read(self, count):
                reads.append(count)
                return b'x' * count

        with mock.patch.object(baseline.os, 'fdopen', side_effect=GrowingSource):
            self.assert_unavailable(baseline.input_fingerprints(self.args))
        self.assertEqual(reads, [len(b'input') + 1] * 3)

    def test_path_replacement_during_read_is_rejected(self):
        real_stat = os.stat
        calls = []

        def changed_path(path):
            info = real_stat(path)
            calls.append(path)
            if len(calls) % 2:
                return info
            copied = {key: getattr(info, key) for key in ('st_dev', 'st_ino', 'st_mode',
                                                         'st_size', 'st_mtime_ns', 'st_ctime_ns')}
            copied['st_ino'] += 1
            return SimpleNamespace(**copied)

        with mock.patch.object(baseline.os, 'stat', side_effect=changed_path):
            self.assert_unavailable(baseline.input_fingerprints(self.args))


class ComparisonTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix='cedar-private-comparison-')
        self.addCleanup(temporary.cleanup)
        self.paths = [Path(temporary.name, f'SECRET-trial-{trial}.json') for trial in (1, 2)]
        rows = LongWindowTests.rows() + LongWindowTests.rows('correction_ready_idle', offset=200_000)
        windows = baseline.idle_windows(rows, LongWindowTests.events(), 200)
        self.reports = []
        for trial in (1, 2):
            self.reports.append({
                'schema_version': 2, 'purpose': 'observational_process_tree_baseline',
                'platform': 'windows', 'status': 'complete', 'driver_exit_code': 0, 'timed_out': False,
                'metadata': {'trial': trial, 'source_commit': 'a' * 40,
                             'workload': 'long_idle_baseline', 'test': baseline.LONG_TEST,
                             'driver_build': 'cargo_test_debug', 'agent_build': 'release',
                             'java_heap_limit_mib': 512, 'java_recipe': 'production_java_xmx512m_unchanged',
                             'jdt_version': '1.61.0', 'jdt_archive_sha256': baseline.JDT_SHA256,
                             'jdt_metadata_basis': 'pinned_expected_archive_verified_by_enclosing_acceptance_script',
                             'input_fingerprint_basis': 'sha256_files_before_launch_not_loaded_image_attestation',
                             'project_cache': 'fresh_generated_project_and_jdt_data', 'os_file_cache': 'uncontrolled',
                             'logical_cpu_count': 4, 'python_version': [3, 12, 1],
                             'input_files': {role: {'sha256': hashlib.sha256(role.encode()).hexdigest(), 'bytes': 42}
                                             for role in baseline.ROLES[:3]}},
                'measurement': {'interval_ms': 200, 'timeout_seconds': 270, 'clock': 'time.perf_counter_ns',
                                'clock_resolution_ns': 100, 'cpu_counter_unit_ns': 100,
                                'cpu': 'per_process_read_midpoint_estimate_percent_of_one_logical_core',
                                'cpu_aggregation': 'sum_of_process_estimates_not_same_window_tree_cpu'},
                'latency_events': [event for event in LongMarkerTests.markers() if 'latency' in event],
                'idle_window_summary': copy.deepcopy(windows)})

    def compare(self):
        for path, report in zip(self.paths, self.reports):
            path.write_text(json.dumps(report))
        return baseline.compare_reports(*self.paths)

    def test_same_binary_comparison_keeps_both_observations_and_no_change_claim(self):
        report = self.compare()
        self.assertEqual(report['status'], 'observed')
        self.assertTrue(report['same_input_files_and_source'])
        self.assertEqual([trial['trial'] for trial in report['trials']], [1, 2])
        self.assertEqual(report['interpretation'], 'descriptive_observations_no_optimization_or_regression_claim')
        public = json.dumps(report)
        for secret in ('SECRET', 'PRIVATE', 'p95', 'regression_percent', 'pid'):
            self.assertNotIn(secret, public)

    def test_different_input_source_workload_dependencies_or_observer_are_not_comparable(self):
        mutations = [('metadata', 'source_commit', 'b' * 40), ('metadata', 'java_heap_limit_mib', 256),
                     ('metadata', 'jdt_archive_sha256', 'f' * 64), ('metadata', 'workload', 'normal_acceptance'),
                     ('metadata', 'test', baseline.PRODUCTION_TEST), ('metadata', 'trial', 1),
                     ('metadata', 'python_version', [3, 13, 0]), ('metadata', 'logical_cpu_count', 8),
                     ('measurement', 'interval_ms', 400), ('measurement', 'timeout_seconds', 240),
                     ('measurement', 'clock', 'time.monotonic_ns'), ('measurement', 'clock_resolution_ns', 1000000)]
        original = copy.deepcopy(self.reports[1])
        for section, field, value in mutations:
            self.reports[1] = copy.deepcopy(original)
            self.reports[1][section][field] = value
            self.assertEqual(self.compare()['status'], 'not_comparable', field)
        self.reports[1] = copy.deepcopy(original)
        self.reports[1]['metadata']['input_files']['agent']['sha256'] = 'f' * 64
        self.assertEqual(self.compare()['status'], 'not_comparable')
        self.reports[1] = copy.deepcopy(original)
        self.reports[1]['metadata']['input_files']['jvm']['bytes'] = 43
        self.assertEqual(self.compare()['status'], 'not_comparable')

    def test_partial_or_failed_observation_is_not_a_resource_failure_or_zero(self):
        self.reports[1]['status'] = 'incomplete'
        self.reports[1]['idle_window_summary'] = baseline.idle_windows([], [], 200)
        self.reports[1]['latency_events'] = []
        report = self.compare()
        self.assertEqual(report['status'], 'observation_incomplete')
        self.assertTrue(report['same_input_files_and_source'])
        self.assertTrue(report['trials'][0]['observation_complete'])
        self.assertFalse(report['trials'][1]['observation_complete'])
        self.assertIsNone(report['trials'][1]['idle_window_summary']['semantic_ready_idle']['rss_bytes']['tree']['median'])

    def test_hostile_nested_text_numbers_and_duplicate_fields_never_escape(self):
        for value in ('SECRET_PATH', True, float('nan'), float('inf'), -1, 10**1000):
            self.reports[1]['idle_window_summary']['semantic_ready_idle']['rss_bytes']['tree']['median'] = value
            report = self.compare()
            self.assertEqual(report['status'], 'not_comparable')
            self.assertNotIn('SECRET', json.dumps(report, allow_nan=False))
        self.paths[1].write_text('{"schema_version":2,"schema_version":2}')
        self.assertEqual(baseline.compare_reports(*self.paths)['status'], 'not_comparable')

    def test_unrelated_text_is_not_copied_and_latency_extras_are_rejected(self):
        self.reports[1]['issues'] = ['SECRET_EXCEPTION']
        self.reports[1]['metadata']['path'] = 'SECRET_PATH'
        self.reports[1]['idle_window_summary']['semantic_ready_idle']['extra'] = {'SECRET': 'SECRET'}
        report = self.compare()
        self.assertEqual(report['status'], 'observed')
        self.assertNotIn('SECRET', json.dumps(report))
        self.reports[1]['latency_events'][0]['text'] = 'SECRET_MESSAGE'
        self.assertEqual(self.compare()['status'], 'not_comparable')

    def test_input_files_are_hashed_without_paths_and_unavailable_is_null(self):
        files = []
        for index in range(3):
            path = self.paths[0].parent / f'SECRET-binary-{index}'
            path.write_bytes(bytes([index]) * (index + 100))
            files.append(path)
        args = SimpleNamespace(driver=files[0], agent=files[1], java=files[2])
        fingerprints, issues = baseline.input_fingerprints(args)
        self.assertFalse(issues)
        for index, role in enumerate(baseline.ROLES[:3]):
            self.assertEqual(fingerprints[role], {'sha256': hashlib.sha256(files[index].read_bytes()).hexdigest(),
                                                'bytes': index + 100})
        self.assertNotIn('SECRET', json.dumps(fingerprints))
        files[2].unlink()
        fingerprints, issues = baseline.input_fingerprints(args)
        self.assertEqual(issues, ['input_fingerprint_unavailable'])
        self.assertEqual(fingerprints['jvm'], {'sha256': None, 'bytes': None})

    def test_compare_cli_preserves_incomplete_observation_and_rejects_mismatch(self):
        output = self.paths[0].parent / 'comparison.json'
        self.reports[1]['status'] = 'incomplete'
        self.compare()
        argv = ['measure_process_tree.py', 'compare', '--trial-1', str(self.paths[0]),
                '--trial-2', str(self.paths[1]), '--output', str(output)]
        with mock.patch.object(baseline.sys, 'argv', argv):
            self.assertEqual(baseline.main(), 0)
        self.assertEqual(json.loads(output.read_text())['status'], 'observation_incomplete')
        self.reports[1]['metadata']['source_commit'] = 'b' * 40
        self.compare()
        with mock.patch.object(baseline.sys, 'argv', argv):
            self.assertEqual(baseline.main(), 1)
        self.assertEqual(json.loads(output.read_text())['status'], 'not_comparable')


class RunFailureTests(unittest.TestCase):
    def test_long_workload_selects_only_exact_fixed_test_and_hashes_before_launch(self):
        with tempfile.TemporaryDirectory(prefix='cedar-resource-long-route-') as root:
            args = SimpleNamespace(driver='/PRIVATE/driver', agent='/PRIVATE/agent',
                                   java='/PRIVATE/java', source_commit='a' * 40,
                                   interval_ms=200, timeout_seconds=270,
                                   workload='long_idle_baseline', trial=2,
                                   phase_file=str(Path(root, 'phases')),
                                   transcript=str(Path(root, 'private')),
                                   output=str(Path(root, 'report.json')))
            backend = FakeBackend()
            child = mock.Mock(pid=100, returncode=42)
            child.poll.return_value = 42
            order = []

            def fingerprint(arguments):
                order.append('hash')
                return {}, []

            def launch(command, **kwargs):
                order.append('launch')
                self.assertEqual(command, [args.driver, baseline.LONG_TEST, '--exact',
                                           '--ignored', '--nocapture', '--test-threads=1'])
                Path(args.phase_file).write_text(''.join(json.dumps(event) + '\n'
                                                         for event in LongMarkerTests.markers()))
                return child

            factory = 'Windows' if sys.platform == 'win32' else 'Linux'
            with mock.patch.object(baseline, factory, return_value=backend), \
                    mock.patch.object(baseline, 'input_fingerprints', side_effect=fingerprint), \
                    mock.patch.object(baseline.subprocess, 'Popen', side_effect=launch), \
                    mock.patch.object(baseline.time, 'perf_counter_ns', backend.clock):
                self.assertEqual(baseline.run(args), 42)
            self.assertEqual(order, ['hash', 'launch'])
            report = json.loads(Path(args.output).read_text())
            self.assertEqual(report['metadata']['workload'], 'long_idle_baseline')
            self.assertEqual(report['metadata']['trial'], 2)
            self.assertEqual([event['latency'] for event in report['latency_events']], list(baseline.LATENCIES))
            self.assertEqual(set(report['phase_summary']), set(baseline.LONG_PHASES))
            self.assertEqual(report['idle_window_summary']['semantic_ready_idle']['status'], 'unknown')
            self.assertNotIn('PRIVATE', json.dumps(report))

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
