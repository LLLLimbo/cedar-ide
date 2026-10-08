#!/usr/bin/env python3
"""Bounded observational sampling of one owned, prebuilt headless acceptance run.

No process names, PIDs, paths, command lines, environment or exception strings
are serialized. Child stdout/stderr stay in the caller's private transcript.
The sampler never decides whether resource consumption passes acceptance.
"""
import argparse
import ctypes
from dataclasses import dataclass
import hashlib
import json
import math
import os
from pathlib import Path
import re
import stat
import statistics
import subprocess
import sys
import time

import collect_gc_control as gc_control

ROLES = ('headless_driver', 'agent', 'jvm', 'other_descendant')
PHASES = ('starting', 'java_initialized', 'semantic_ready_idle', 'query_workload',
          'cleanup', 'complete')
LONG_PHASES = (*PHASES[:4], 'correction_ready_idle', 'closing', *PHASES[4:])
LATENCIES = ('java_initialize', 'open_exact_diagnostics', 'definition_confined_uri',
             'completion', 'deferred_import_resolve', 'editor_apply_undo_redo',
             'correction_exact_diagnostics', 'stop_verified_root_exit')
LATENCY_PHASES = ('starting', 'java_initialized', *('query_workload',) * 5, 'cleanup')
IDLE_PHASES = ('semantic_ready_idle', 'correction_ready_idle')
LONG_WORKLOADS = ('long_idle_baseline', 'gc_diagnostic_control')
WORKLOADS = ('normal_acceptance', *LONG_WORKLOADS)
JDT_SHA256 = '338e7e73d61836651ba2453919a0d34fa763eb4e7c03342092309bffb8934c64'
LIVE_METRICS = ('rss_bytes', 'threads', 'handles')
CPU_SUM = 'cpu_percent_one_core_sum_of_process_estimates'
METRICS = (*LIVE_METRICS, CPU_SUM)
CPU_ISSUES = frozenset(('cpu_timing_invalid', 'cpu_counter_invalid'))
CPU_INTERVAL_FIELDS = ('cpu_delta_ns', 'elapsed_ns', 'elapsed_min_ns', 'elapsed_max_ns',
                       'previous_read_span_ns', 'current_read_span_ns',
                       'percent_lower_from_timing', 'percent_upper_from_timing',
                       'previous_read_start_elapsed_ns', 'previous_read_end_elapsed_ns',
                       'current_read_start_elapsed_ns', 'current_read_end_elapsed_ns')
MAX_PROCESSES = 64
MAX_SAMPLES = 1600
MAX_SYSTEM_PROCESSES = 32768
MAX_MARKER_BYTES = 8192
MAX_INPUT_FILE_BYTES = 512 * 1024 * 1024
PRODUCTION_TEST = ('language_ui::real_java_tests::acceptance::windows::production::'
                   'real_windows_normal_agent_java_editor_acceptance')
LONG_TEST = ('language_ui::real_java_tests::acceptance::windows::production::'
             'real_windows_normal_agent_java_resource_baseline')
GC_TEST = ('language_ui::real_java_tests::acceptance::windows::production::'
           'real_windows_java_gc_diagnostic_control')
GC_SELECTION_FILE = '.cedar-java-gc-selection-private.json'
ISSUES = frozenset(('discovery_failed', 'discovery_limit', 'identity_or_cpu_unavailable',
                   'descendant_unavailable', 'image_unavailable', 'identity_changed',
                   'liveness_unavailable', 'sample_raced_exit', 'sample_unavailable',
                   'root_image_mismatch', 'process_limit', 'metric_unavailable',
                   'parent_identity_unavailable', 'observation_failed')) | CPU_ISSUES


class ObservationError(Exception):
    """A fixed classification, never an OS exception message."""


def issue(error):
    return str(error) if str(error) in ISSUES else 'observation_failed'


def path_key(path):
    value = os.path.normcase(os.path.realpath(path))
    if value.startswith('\\\\?\\'):
        value = value[4:]
    return value


@dataclass
class Process:
    pid: int
    created: int
    image: str
    native: object = None
    role: str = 'other_descendant'
    instance: int = 0
    previous_cpu: object = None
    previous_read: object = None
    previous_phase: object = None
    exited: bool = False
    exit_time: object = None


class Windows:
    """Query-only retained handles pin identities, including after reparenting."""
    handle_metric = 'windows_process_handle_count'
    cpu_counter_unit_ns = 100  # Representation unit, not guaranteed accuracy.

    def __init__(self, clock=None):
        self.clock = clock or time.perf_counter_ns
        from ctypes import wintypes as w
        self.w = w
        self.k = ctypes.WinDLL('kernel32', use_last_error=True)
        self.p = ctypes.WinDLL('psapi', use_last_error=True)

        class Entry(ctypes.Structure):
            _fields_ = [('size', w.DWORD), ('usage', w.DWORD), ('pid', w.DWORD),
                        ('heap', ctypes.c_size_t), ('module', w.DWORD), ('threads', w.DWORD),
                        ('parent', w.DWORD), ('priority', w.LONG), ('flags', w.DWORD),
                        ('image', w.WCHAR * 260)]

        class Memory(ctypes.Structure):
            _fields_ = [('size', w.DWORD), ('faults', w.DWORD)] + [
                (key, ctypes.c_size_t) for key in ('peak', 'rss', 'peak_paged', 'paged',
                                                 'peak_nonpaged', 'nonpaged', 'pagefile',
                                                 'peak_pagefile')]

        self.Entry, self.Memory = Entry, Memory
        signatures = {
            'OpenProcess': ([w.DWORD, w.BOOL, w.DWORD], w.HANDLE),
            'CloseHandle': ([w.HANDLE], w.BOOL),
            'CreateToolhelp32Snapshot': ([w.DWORD, w.DWORD], w.HANDLE),
            'Process32FirstW': ([w.HANDLE, ctypes.POINTER(Entry)], w.BOOL),
            'Process32NextW': ([w.HANDLE, ctypes.POINTER(Entry)], w.BOOL),
            'GetProcessTimes': ([w.HANDLE] + [ctypes.POINTER(w.FILETIME)] * 4, w.BOOL),
            'WaitForSingleObject': ([w.HANDLE, w.DWORD], w.DWORD),
            'GetProcessHandleCount': ([w.HANDLE, ctypes.POINTER(w.DWORD)], w.BOOL),
            'QueryFullProcessImageNameW': ([w.HANDLE, w.DWORD, w.LPWSTR,
                                          ctypes.POINTER(w.DWORD)], w.BOOL),
        }
        for name, (args, result) in signatures.items():
            function = getattr(self.k, name)
            function.argtypes, function.restype = args, result
        self.p.GetProcessMemoryInfo.argtypes = [w.HANDLE, ctypes.POINTER(Memory), w.DWORD]
        self.p.GetProcessMemoryInfo.restype = w.BOOL

    def scan(self):
        handle = self.k.CreateToolhelp32Snapshot(2, 0)
        if handle == ctypes.c_void_p(-1).value:
            raise ObservationError('discovery_failed')
        result = {}
        try:
            entry = self.Entry()
            entry.size = ctypes.sizeof(entry)
            ok = self.k.Process32FirstW(handle, ctypes.byref(entry))
            while ok:
                if len(result) >= MAX_SYSTEM_PROCESSES:
                    raise ObservationError('discovery_limit')
                result[entry.pid] = (entry.parent, entry.threads, None)
                ok = self.k.Process32NextW(handle, ctypes.byref(entry))
            if ctypes.get_last_error() != 18:  # ERROR_NO_MORE_FILES
                raise ObservationError('discovery_failed')
            return result
        finally:
            self.k.CloseHandle(handle)

    def times(self, handle):
        values = [self.w.FILETIME() for _ in range(4)]
        if not self.k.GetProcessTimes(handle, *(ctypes.byref(v) for v in values)):
            raise ObservationError('identity_or_cpu_unavailable')
        return [(v.dwHighDateTime << 32) | v.dwLowDateTime for v in values]

    def open(self, pid):
        # Query + VM_READ for working set, synchronization for a zero-time wait.
        handle = self.k.OpenProcess(0x0400 | 0x0010 | 0x100000, False, pid)
        if not handle:
            raise ObservationError('descendant_unavailable')
        try:
            created, _, _, _ = self.times(handle)
            image = ctypes.create_unicode_buffer(32768)
            length = self.w.DWORD(len(image))
            if not self.k.QueryFullProcessImageNameW(handle, 0, image, ctypes.byref(length)):
                raise ObservationError('image_unavailable')
            return Process(pid, created, path_key(image.value), handle)
        except BaseException:
            self.k.CloseHandle(handle)
            raise

    def read(self, process, scan):
        # CPython 3.12 Windows monotonic_ns uses coarse GetTickCount64;
        # perf_counter_ns uses QPC. Bracket only the actual CPU counter query.
        cpu_start = self.clock()
        created, exited_at, kernel, user = self.times(process.native)
        cpu_end = self.clock()
        cpu = {'cpu_ns': (kernel + user) * 100,
               'cpu_read_start_ns': cpu_start, 'cpu_read_end_ns': cpu_end}
        if created != process.created:
            raise ObservationError('identity_changed')
        wait = self.k.WaitForSingleObject(process.native, 0)
        if wait not in (0, 258):
            raise ObservationError('liveness_unavailable')
        process.exit_time = exited_at or None
        if wait == 0:
            process.exited = True
            return {'rss_bytes': 0, **cpu, 'threads': 0, 'handles': 0}
        memory = self.Memory()
        memory.size = ctypes.sizeof(memory)
        rss = memory.rss if self.p.GetProcessMemoryInfo(
            process.native, ctypes.byref(memory), memory.size) else None
        count = self.w.DWORD()
        handles = count.value if self.k.GetProcessHandleCount(
            process.native, ctypes.byref(count)) else None
        # Recheck liveness after reading; never turn a raced exit into zeros.
        if self.k.WaitForSingleObject(process.native, 0) != 258:
            raise ObservationError('sample_raced_exit')
        return {'rss_bytes': rss, **cpu,
                'threads': scan.get(process.pid, (None, None, None))[1], 'handles': handles}

    def owns_child(self, parent, child):
        created, exited, _, _ = self.times(parent.native)
        return (created == parent.created and created <= child.created
                and (not exited or child.created <= exited))

    def close(self, process):
        self.k.CloseHandle(process.native)


class Linux:
    """Supporting /proc backend; file descriptors are explicitly not Win32 handles."""
    handle_metric = 'linux_open_file_descriptor_count'

    def __init__(self, clock=None):
        self.clock = clock or time.perf_counter_ns
        self.tick_ns = 1_000_000_000 / os.sysconf('SC_CLK_TCK')
        self.cpu_counter_unit_ns = self.tick_ns
        self.page_bytes = os.sysconf('SC_PAGE_SIZE')

    @staticmethod
    def stat(pid):
        data = Path('/proc', str(pid), 'stat').read_text()
        values = data[data.rfind(')') + 2:].split()
        return {'parent': int(values[1]), 'created': int(values[19]),
                'cpu': int(values[11]) + int(values[12]), 'threads': int(values[17]),
                'rss': int(values[21]), 'zombie': values[0] == 'Z'}

    def scan(self):
        result = {}
        for entry in Path('/proc').iterdir():
            if not entry.name.isdecimal():
                continue
            if len(result) >= MAX_SYSTEM_PROCESSES:
                raise ObservationError('discovery_limit')
            try:
                value = self.stat(int(entry.name))
                result[int(entry.name)] = (value['parent'], value['threads'], value['created'])
            except (FileNotFoundError, ProcessLookupError):
                continue  # Unrelated system process disappeared during enumeration.
            except (OSError, ValueError, IndexError):
                raise ObservationError('discovery_failed') from None
        return result

    def open(self, pid):
        try:
            before = self.stat(pid)
            image = path_key(os.readlink(Path('/proc', str(pid), 'exe')))
            if self.stat(pid)['created'] != before['created']:
                raise ObservationError('identity_changed')
            return Process(pid, before['created'], image)
        except (OSError, ValueError, IndexError):
            raise ObservationError('descendant_unavailable') from None

    def read(self, process, scan):
        try:
            before = self.stat(process.pid)
            if before['created'] != process.created:
                raise ObservationError('identity_changed')
            try:
                handles = sum(1 for _ in Path('/proc', str(process.pid), 'fd').iterdir())
            except OSError:
                handles = None
            cpu_start = self.clock()
            after = self.stat(process.pid)
            cpu_end = self.clock()
            if after['created'] != process.created or after['zombie'] != before['zombie']:
                raise ObservationError('sample_raced_exit')
            process.exited = after['zombie']
            return {'rss_bytes': 0 if process.exited else after['rss'] * self.page_bytes,
                    'cpu_ns': round(after['cpu'] * self.tick_ns),
                    'cpu_read_start_ns': cpu_start, 'cpu_read_end_ns': cpu_end,
                    'threads': 0 if process.exited else after['threads'],
                    'handles': 0 if process.exited else handles}
        except (FileNotFoundError, ProcessLookupError):
            # No retained kernel handle: final CPU and exit identity are unknown.
            raise ObservationError('sample_raced_exit') from None
        except (OSError, ValueError, IndexError):
            raise ObservationError('sample_unavailable') from None

    def owns_child(self, parent, child):
        try:
            value = self.stat(parent.pid)
            return value['created'] == parent.created and parent.created <= child.created
        except (OSError, ValueError, IndexError):
            raise ObservationError('parent_identity_unavailable') from None

    @staticmethod
    def close(process):
        pass


def marker_events(path, workload='normal_acceptance'):
    """Allowlist typed records; never retain arbitrary marker file contents."""
    try:
        with open(path, 'rb') as source:
            data = source.read(MAX_MARKER_BYTES + 1)
    except FileNotFoundError:
        return [], [], []
    except OSError:
        return [], [], ['phase_unavailable']
    if len(data) > MAX_MARKER_BYTES:
        return [], [], ['phase_limit']
    phases = LONG_PHASES if workload in LONG_WORKLOADS else PHASES
    events, latencies, previous_elapsed = [], [], 0
    try:
        # A partial final write is retried next sample.
        for line in data.splitlines(keepends=True):
            if not line.endswith(b'\n'):
                continue
            value = json.loads(line, object_pairs_hook=unique_object)
            if (not isinstance(value, dict)
                    or type(value['elapsed_ms']) is not int
                    or not 0 <= value['elapsed_ms'] <= 300_000
                    or value['elapsed_ms'] < previous_elapsed):
                raise ValueError()
            previous_elapsed = value['elapsed_ms']
            if set(value) == {'phase', 'elapsed_ms'}:
                if len(events) >= len(phases) or value['phase'] != phases[len(events)]:
                    raise ValueError()
                events.append(value)
            elif (workload in LONG_WORKLOADS
                  and set(value) == {'latency', 'elapsed_ms', 'duration_ns'}):
                if (len(latencies) >= len(LATENCIES)
                        or value['latency'] != LATENCIES[len(latencies)]
                        or not events or events[-1]['phase'] != LATENCY_PHASES[len(latencies)]
                        or type(value['duration_ns']) is not int
                        or not 0 <= value['duration_ns'] <= 300_000_000_000):
                    raise ValueError()
                latencies.append(value)
            else:
                raise ValueError()
    except (ValueError, KeyError, TypeError, RecursionError, UnicodeError):
        return [], [], ['phase_invalid']
    return events, latencies, []


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError()
        result[key] = value
    return result


def phase_events(path):
    events, _, issues = marker_events(path)
    return events, issues


def aggregate(records, complete):
    """Live resources share a sweep; CPU estimates have independent intervals."""
    result = {}
    for metric in METRICS:
        source = 'cpu_percent_one_core' if metric == CPU_SUM else metric
        result[metric] = (sum(row[source] for row in records)
                          if complete and records and all(row[source] is not None for row in records)
                          else None)
    return result


def empty_cpu_interval(status):
    return {'status': status, **dict.fromkeys(CPU_INTERVAL_FIELDS)}


def cpu_interval(process, values, phase, resolution_ns):
    """Estimate CPU using this identity's read brackets, never a sweep timestamp.

    The interval bounds model read placement with clock-resolution assumptions.
    Physical clock accuracy and OS CPU accounting error/quantization are not
    included; these are not confidence bounds for actual CPU consumption.
    """
    result = empty_cpu_interval('unavailable')
    cpu = values.get('cpu_ns')
    start, end = values.get('cpu_read_start_ns'), values.get('cpu_read_end_ns')
    previous_cpu, previous_read, previous_phase = (process.previous_cpu,
                                                   process.previous_read,
                                                   process.previous_phase)
    # A failed observation must not let the next interval bridge unknown data.
    process.previous_cpu = process.previous_read = process.previous_phase = None
    if type(cpu) is not int or cpu < 0:
        result['status'] = 'counter_invalid'
        return None, result, 'cpu_counter_invalid'
    if (type(start) is not int or type(end) is not int or end < start
            or type(resolution_ns) is not int or resolution_ns <= 0):
        result['status'] = 'clock_invalid'
        return None, result, 'cpu_timing_invalid'
    result['current_read_span_ns'] = end - start
    if previous_read is not None:
        result['previous_read_span_ns'] = previous_read[1] - previous_read[0]
    if previous_cpu is None or previous_phase != phase:
        result['status'] = 'warming_up' if previous_cpu is None else 'phase_boundary'
    elif cpu < previous_cpu:
        result['status'] = 'counter_regressed'
        return None, result, 'cpu_counter_invalid'
    else:
        # Each timestamp can be quantized by one clock-resolution unit. A
        # midpoint estimate uses both brackets and includes any scheduler pause.
        minimum = start - previous_read[1] - 2 * resolution_ns
        maximum = end - previous_read[0] + 2 * resolution_ns
        elapsed = ((start - previous_read[0]) + (end - previous_read[1])) / 2
        result.update(cpu_delta_ns=cpu - previous_cpu, elapsed_ns=elapsed,
                      elapsed_min_ns=minimum, elapsed_max_ns=maximum)
        if minimum <= 0 or maximum < minimum:
            result['status'] = 'interval_unresolved'
            return None, result, 'cpu_timing_invalid'
        result.update(status='estimated',
                      percent_lower_from_timing=100 * (cpu - previous_cpu) / maximum,
                      percent_upper_from_timing=100 * (cpu - previous_cpu) / minimum)
    process.previous_cpu, process.previous_read = cpu, (start, end)
    process.previous_phase = phase
    percent = (100 * result['cpu_delta_ns'] / result['elapsed_ns']
               if result['status'] == 'estimated' else None)
    return percent, result, None


class Sampler:
    def __init__(self, backend, root_pid, roles, resolution_ns=None, origin_ns=None):
        self.backend = backend
        self.origin_ns = origin_ns
        self.resolution_ns = (math.ceil(time.get_clock_info('perf_counter').resolution * 1e9)
                              if resolution_ns is None else resolution_ns)
        if set(roles) - set(ROLES):
            raise ObservationError('root_image_mismatch')
        self.required_roles = set(roles)
        self.roles = {path_key(path): role for role, path in roles.items()}
        self.processes = []
        self.issues = set()
        self.root_pid = root_pid
        self.add(backend.open(root_pid), 'headless_driver')

    def add(self, process, role=None):
        process.role = role or self.roles.get(process.image, 'other_descendant')
        if role and self.roles.get(process.image) != role:
            self.backend.close(process)
            raise ObservationError('root_image_mismatch')
        process.instance = len(self.processes) + 1
        self.processes.append(process)

    def sample(self, phase):
        issues = set()
        try:
            scan = self.backend.scan()
        except ObservationError as error:
            scan = {}
            issues.add(issue(error))
        # Keep all admitted identities after parent exit/reparenting. Discover
        # iteratively so grandchildren from this same snapshot are included.
        considered = {p.pid for p in self.processes
                      if scan.get(p.pid, (None, None, None))[2] in (None, p.created)}
        changed = True
        while changed:
            changed = False
            for pid, (parent, _, created) in scan.items():
                if pid in considered:
                    continue
                parents = [p for p in self.processes if p.pid == parent]
                if not parents:
                    continue
                considered.add(pid)
                if len(self.processes) >= MAX_PROCESSES:
                    issues.add('process_limit')
                    continue
                try:
                    process = self.backend.open(pid)
                    if created is not None and created != process.created:
                        self.backend.close(process)
                        raise ObservationError('identity_changed')
                    # An exited/reused parent PID must not adopt a new process.
                    try:
                        belongs = any(self.backend.owns_child(p, process) for p in parents)
                    except BaseException:
                        self.backend.close(process)
                        raise
                    if not belongs:
                        self.backend.close(process)
                        continue
                    self.add(process)
                    changed = True
                except ObservationError as error:
                    issues.add(issue(error))
        rows = []
        for process in self.processes:
            if process.exited:
                continue
            row = {'instance': process.instance, 'role': process.role,
                   'identity_verified': True, 'exited': False,
                   **dict.fromkeys((*LIVE_METRICS, 'cpu_percent_one_core')),
                   'cpu_interval': empty_cpu_interval('unavailable')}
            try:
                values = self.backend.read(process, scan)
                row.update({key: values[key] for key in LIVE_METRICS})
                row['exited'] = process.exited
                previous_read = process.previous_read
                percent, interval, cpu_issue = cpu_interval(process, values, phase, self.resolution_ns)
                if interval['status'] == 'estimated' and self.origin_ns is not None:
                    for key, value in zip(CPU_INTERVAL_FIELDS[-4:],
                                          (*previous_read, values['cpu_read_start_ns'],
                                           values['cpu_read_end_ns'])):
                        interval[key] = value - self.origin_ns
                row['cpu_percent_one_core'], row['cpu_interval'] = percent, interval
                if cpu_issue:
                    issues.add(cpu_issue)
                if any(values[key] is None for key in LIVE_METRICS):
                    issues.add('metric_unavailable')
            except ObservationError as error:
                issues.add(issue(error))
                row['identity_verified'] = False
                process.previous_cpu = None
            rows.append(row)
        # A clock problem invalidates CPU alone, not independently read RSS.
        discovery_complete = not (issues - CPU_ISSUES)
        known_roles = {process.role for process in self.processes}
        roles_observed = self.required_roles <= known_roles
        totals = aggregate(rows, discovery_complete and roles_observed)
        complete = (discovery_complete and roles_observed
                    and all(value is not None for value in totals.values()))
        by_role = {}
        for role in ROLES:
            role_rows = [row for row in rows if row['role'] == role]
            known = [process for process in self.processes if process.role == role]
            by_role[role] = (dict.fromkeys(METRICS, 0) if not role_rows and known
                             and all(process.exited for process in known) and discovery_complete
                             else aggregate(role_rows, discovery_complete))
        self.issues.update(issues)
        return {'phase': phase, 'complete': complete, 'issues': sorted(issues), 'processes': rows,
                'required_roles_observed': roles_observed, 'phase_stable': True,
                'aggregate': totals,
                'metric_complete': {key: value is not None for key, value in totals.items()},
                'by_role': by_role}

    def validate_phase(self, sample, phase_after, marker_issues):
        if phase_after == sample['phase'] and not marker_issues:
            return
        sample['phase_stable'] = False
        sample['complete'] = False
        sample['metric_complete'][CPU_SUM] = False
        sample['aggregate'][CPU_SUM] = None
        for row in sample['processes']:
            row['cpu_percent_one_core'] = None
            row['cpu_interval'] = empty_cpu_interval('phase_unstable')
        for values in sample['by_role'].values():
            values[CPU_SUM] = None
        # Neither endpoint of an accepted CPU interval may straddle a marker.
        for process in self.processes:
            process.previous_phase = None

    def close(self):
        for process in self.processes:
            self.backend.close(process)

    def corroborate_gc_selection(self, root, selection_path, root_info):
        """Use retained identities before handle closure; publish no raw witness fields.

        The private driver witness attests natural/protocol cleanup. This check
        independently corroborates its selected identity and observed exit only.
        It does not authenticate the witness or establish a GC/acceptance pass.
        """
        if self.backend.handle_metric != 'windows_process_handle_count':
            return gc_selection_status('platform_unsupported')
        try:
            selection, _, _, selection_sha256 = gc_control.read_selection(
                root, selection_path, root_info, gc_control.LIMITS)
        except (OSError, ValueError, TypeError, RecursionError):
            return gc_selection_status('selection_rejected')
        candidates = [process for process in self.processes if process.role == 'jvm']
        if len(candidates) != 1:
            return gc_selection_status('jvm_count_mismatch')
        process = candidates[0]
        if (process.pid != selection['pid']
                or process.created != selection['creation_time_100ns_since_1601']
                or self.roles.get(process.image) != 'jvm'):
            return gc_selection_status('jvm_identity_mismatch')
        # The driver may have completed between the final sample and poll.
        # Refresh only this retained identity, with no PID reopen or adoption.
        # These teardown reads never enter the CPU/idle sample calculations.
        try:
            self.backend.read(process, {})
        except (OSError, ValueError, ObservationError):
            return gc_selection_status('jvm_exit_unobserved')
        if not process.exited:
            return gc_selection_status('jvm_exit_unobserved')
        return gc_selection_status('corroborated', selection_sha256)


def gc_selection_status(status, selection_sha256=None):
    return {'status': status, 'identity_corroborated': status == 'corroborated',
            'selection_sha256': selection_sha256}


def prepare_gc_selection(owned_root, selection_path):
    """Pin the checked directory before launch; only the driver creates the file."""
    root, selection = Path(owned_root), Path(selection_path)
    if '..' in root.parts or '..' in selection.parts:
        raise ValueError()
    root = root.absolute()
    if not selection.is_absolute():
        selection = root / selection
    if selection.parent != root or selection.name != GC_SELECTION_FILE:
        raise ValueError()
    root_info = gc_control.check_components(root)
    if not stat.S_ISDIR(root_info.st_mode):
        raise ValueError()
    try:
        selection.lstat()
    except FileNotFoundError:
        return root, selection, root_info
    raise ValueError()


def summary(samples, phases=PHASES):
    result = {}
    for phase in phases:
        rows = [sample for sample in samples if sample['phase'] == phase
                and sample.get('phase_stable', False)]
        result[phase] = {'samples': len(rows),
                         'complete_samples': sum(row['complete'] for row in rows)}
        for metric in METRICS:
            values = [row['aggregate'][metric] for row in rows
                      if row['aggregate'][metric] is not None]
            result[phase][metric] = {'observed_max': max(values) if values else None,
                                     'valid_samples': len(values)}
    return result


def input_fingerprints(args):
    """Hash selected files before launch; this does not attest loaded images."""
    def identity(info):
        # Windows path stat infers executable bits from .exe; handle fstat
        # does not. Windows Python 3.12 also gives path stat a birthtime ctime,
        # while handle fstat exposes ChangeTime. Compare common identity here;
        # verify ctime stability separately within each API family below.
        return (info.st_dev, info.st_ino, stat.S_IFMT(info.st_mode), info.st_size,
                info.st_mtime_ns)

    result, issues = {}, []
    for role in ROLES[:3]:
        path = getattr(args, {'headless_driver': 'driver', 'agent': 'agent', 'jvm': 'java'}[role])
        digest, size = hashlib.sha256(), 0
        try:
            candidate = os.stat(path)
            if not stat.S_ISREG(candidate.st_mode) or not 0 < candidate.st_size <= MAX_INPUT_FILE_BYTES:
                raise ValueError()
            # Nonblocking open prevents a raced replacement by a FIFO from
            # waiting for a writer. Windows binary mode preserves input bytes.
            descriptor = os.open(path, os.O_RDONLY | getattr(os, 'O_NONBLOCK', 0)
                                 | getattr(os, 'O_BINARY', 0))
            with os.fdopen(descriptor, 'rb') as source:
                before = os.fstat(source.fileno())
                if not stat.S_ISREG(before.st_mode) or identity(before) != identity(candidate):
                    raise ValueError()
                remaining = before.st_size + 1
                while remaining:
                    block = source.read(min(1024 * 1024, remaining))
                    if not block:
                        break
                    size += len(block)
                    remaining -= len(block)
                    digest.update(block)
                after = os.fstat(source.fileno())
            final_path = os.stat(path)
            if (identity(before) != identity(after) or identity(after) != identity(final_path)
                    or before.st_ctime_ns != after.st_ctime_ns
                    or candidate.st_ctime_ns != final_path.st_ctime_ns
                    or size != before.st_size):
                raise ValueError()
            result[role] = {'sha256': digest.hexdigest(), 'bytes': size}
        except (OSError, ValueError):
            result[role] = {'sha256': None, 'bytes': None}
            issues.append('input_fingerprint_unavailable')
    return result, issues


def rss_distribution(rows, role, covered):
    values = [(row['aggregate'] if role is None else row['by_role'][role])['rss_bytes']
              for row in rows]
    valid = [value for value in values if value is not None]
    known = covered and bool(values) and len(valid) == len(values)
    return {'observed_samples': len(values), 'valid_samples': len(valid),
            'unknown_samples': len(values) - len(valid),
            'median': statistics.median(valid) if known else None,
            'observed_min': min(valid) if known else None,
            'observed_max': max(valid) if known else None}


def integrated_cpu(rows, start, end, tolerance, covered):
    """Keep per-identity counter intervals independent, including in sums.

    Only whole read brackets inside this observed window are included. Neither
    CPU counters nor RSS are interpolated to the requested ten-second edges.
    """
    instances = sorted({(item['instance'], item['role'])
                        for row in rows for item in row['processes']})
    result = []
    for instance, role in instances:
        intervals = []
        present, invalid = 0, 0
        for row in rows:
            for item in row['processes']:
                if item['instance'] != instance:
                    continue
                present += 1
                value = item['cpu_interval']
                endpoints = [value.get(key) for key in CPU_INTERVAL_FIELDS[-4:]]
                valid = (item['identity_verified'] and value['status'] == 'estimated'
                         and all(type(point) is int for point in endpoints))
                invalid += not valid
                if (valid and start <= endpoints[0] <= endpoints[1] < endpoints[2] <= endpoints[3] <= end):
                    intervals.append(value)
        gaps = sum((before['current_read_start_elapsed_ns'], before['current_read_end_elapsed_ns'])
                   != (after['previous_read_start_elapsed_ns'], after['previous_read_end_elapsed_ns'])
                   for before, after in zip(intervals, intervals[1:]))
        first = intervals[0]['previous_read_start_elapsed_ns'] if intervals else None
        last = intervals[-1]['current_read_end_elapsed_ns'] if intervals else None
        cpu = sum(value['cpu_delta_ns'] for value in intervals) if intervals else None
        elapsed = sum(value['elapsed_ns'] for value in intervals) if intervals else None
        lower = sum(value['elapsed_min_ns'] for value in intervals) if intervals else None
        upper = sum(value['elapsed_max_ns'] for value in intervals) if intervals else None
        known = (covered and bool(intervals) and present == len(rows) and not gaps and not invalid
                 and first - start <= tolerance and end - last <= tolerance)
        result.append({'instance': instance, 'role': role,
                       'status': 'observed' if known else 'unknown',
                       'interval_count': len(intervals), 'discontinuities': gaps, 'unknown_samples': invalid,
                       'first_read_elapsed_ns': first, 'last_read_elapsed_ns': last,
                       'observed_cpu_delta_ns': cpu, 'observed_interval_elapsed_ns': elapsed,
                       'percent_one_core': 100 * cpu / elapsed if known else None,
                       'percent_lower_from_timing': 100 * cpu / upper if known else None,
                       'percent_upper_from_timing': 100 * cpu / lower if known else None})
    all_known = (covered and set(ROLES[:3]) <= {item['role'] for item in result}
                 and all(item['status'] == 'observed' for item in result))
    def scope(role=None):
        items = [item for item in result if role is None or item['role'] == role]
        known = all_known if role is None else covered and bool(items) and all(
            item['status'] == 'observed' for item in items)
        return {'independent_process_count': len(items),
                'sum_of_independent_cpu_deltas_ns': sum(item['observed_cpu_delta_ns'] for item in items)
                if known else None,
                'sum_of_independent_process_estimates_percent_one_core': sum(item['percent_one_core'] for item in items)
                if known else None}
    return {'interval_basis': 'independent_per_identity_whole_read_intervals_no_edge_interpolation',
            'tree': scope(), 'by_role': {role: scope(role) for role in ROLES},
            'processes': result}


def idle_windows(samples, events, interval_ms):
    """Last ten seconds observed within each completed thirty-second phase.

    Driver marker times and sampler times have different origins. The window is
    anchored to the final stable sweep, not to an invented shared clock epoch.
    A two-cadence coverage allowance tolerates endpoint placement, not missing
    metrics. Coverage is an observation description, never an acceptance gate.
    """
    result = {}
    durations = {event['phase']: after['elapsed_ms'] - event['elapsed_ms']
                 for event, after in zip(events, events[1:])}
    tolerance = 2 * interval_ms * 1_000_000
    for phase in IDLE_PHASES:
        phase_rows = [row for row in samples if row['phase'] == phase]
        stable = [row for row in phase_rows if row['phase_stable']]
        end = stable[-1]['sweep_end_elapsed_ns'] if stable else None
        start = end - 10_000_000_000 if end is not None else None
        rows = [row for row in stable if row['sweep_start_elapsed_ns'] >= start] if stable else []
        first = rows[0]['sweep_start_elapsed_ns'] if rows else None
        span = end - first if rows else None
        gaps = [after['sweep_start_elapsed_ns'] - before['sweep_end_elapsed_ns']
                for before, after in zip(rows, rows[1:])]
        maximum_gap = max(gaps) if gaps else None
        unstable = sum(not row['phase_stable'] and row['sweep_end_elapsed_ns'] >= start
                       and row['sweep_start_elapsed_ns'] <= end
                       for row in phase_rows) if rows else 0
        following = [row['sweep_end_elapsed_ns'] for row in samples
                     if end is not None and row['sweep_end_elapsed_ns'] >= end
                     and LONG_PHASES.index(row.get('observed_phase_after_sweep', row['phase']))
                     > LONG_PHASES.index(phase)]
        trailing_gap = min(following) - end if following else None
        covered = (len(rows) >= 2 and durations.get(phase, 0) >= 30_000
                   and first - start <= tolerance and maximum_gap <= tolerance
                   and trailing_gap is not None and trailing_gap <= tolerance
                   and all(row['sweep_end_elapsed_ns'] - row['sweep_start_elapsed_ns'] <= tolerance
                           for row in rows) and not unstable)
        result[phase] = {
            'status': 'observed' if covered else 'unknown',
            'requested_phase_ms': 30_000, 'observed_marker_duration_ms': durations.get(phase),
            'window_basis': 'final_phase_stable_sweep_end_minus_ten_seconds_sampler_clock',
            'requested_window_ms': 10_000, 'requested_start_elapsed_ns': start,
            'observed_end_elapsed_ns': end, 'first_sample_elapsed_ns': first,
            'observed_sample_span_ns': span, 'maximum_unsampled_gap_ns': maximum_gap,
            'trailing_phase_observation_gap_ns': trailing_gap,
            'coverage_allowance_ns': tolerance, 'stable_samples': len(rows),
            'unstable_samples': unstable,
            'rss_bytes': {'tree': rss_distribution(rows, None, covered),
                          'by_role': {role: rss_distribution(rows, role, covered) for role in ROLES}},
            'cpu': integrated_cpu(rows, start, end, tolerance, covered)}
    return result


def run(args):
    workload = getattr(args, 'workload', 'normal_acceptance')
    trial = getattr(args, 'trial', None)
    gc_diagnostic = workload == 'gc_diagnostic_control'
    phases = LONG_PHASES if workload in LONG_WORKLOADS else PHASES
    test = (GC_TEST if gc_diagnostic else
            LONG_TEST if workload == 'long_idle_baseline' else PRODUCTION_TEST)
    backend = Windows() if sys.platform == 'win32' else Linux()
    resolution_ns = math.ceil(time.get_clock_info('perf_counter').resolution * 1e9)
    fingerprints, fingerprint_issues = input_fingerprints(args)
    report = {
        'schema_version': 2, 'purpose': 'observational_process_tree_baseline',
        'status': 'incomplete', 'acceptance_evaluated': False,
        'platform': 'windows' if sys.platform == 'win32' else 'linux',
        'metadata': {'source_commit': args.source_commit, 'driver_build': 'cargo_test_debug',
                     'workload': workload, 'trial': trial, 'test': test,
                     'input_files': fingerprints,
                     'input_fingerprint_basis': 'sha256_files_before_launch_not_loaded_image_attestation',
                     'java_recipe': 'production_java_xmx512m_unchanged', 'java_heap_limit_mib': 512,
                     'jdt_archive_sha256': JDT_SHA256,
                     'jdt_metadata_basis': 'pinned_expected_archive_verified_by_enclosing_acceptance_script',
                     'agent_build': 'release', 'project_cache': 'fresh_generated_project_and_jdt_data',
                     'os_file_cache': 'uncontrolled', 'java_launch': 'same_production_acceptance_run',
                     'jdt_version': '1.61.0', 'logical_cpu_count': os.cpu_count(),
                     'python_version': list(sys.version_info[:3]),
                     'run_metadata_artifact': 'cedar-windows-java-acceptance.txt'},
        'measurement': {'interval_ms': args.interval_ms, 'timeout_seconds': args.timeout_seconds,
                        'maximum_processes': MAX_PROCESSES, 'maximum_samples': MAX_SAMPLES,
                        'rss': 'sum_of_sampled_resident_working_sets_may_double_count_shared_pages',
                        'clock': 'time.perf_counter_ns', 'clock_resolution_ns': resolution_ns,
                        'cpu': 'per_process_read_midpoint_estimate_percent_of_one_logical_core',
                        'cpu_aggregation': 'sum_of_process_estimates_not_same_window_tree_cpu',
                        'cpu_timing_bounds': 'read_placement_with_clock_resolution_excludes_clock_accuracy_and_cpu_accounting_error',
                        'cpu_counter_unit_ns': backend.cpu_counter_unit_ns,
                        'handles': backend.handle_metric,
                        'discovery': 'sampled_descendants_short_lived_children_may_be_missed',
                        'idle': ('thirty_seconds_after_initial_and_correction_diagnostics_background_work_may_continue'
                                 if workload in LONG_WORKLOADS else
                                 'two_seconds_after_exact_initial_diagnostics_background_work_may_continue'),
                        'excludes': ['sampler', 'cargo_and_compiler', 'gui_rendering', 'ssh',
                                     'earlier_acceptance_runs', 'system_services']},
        'issues': [], 'phase_events': [], 'latency_events': [], 'samples': [], 'phase_summary': {},
        'idle_window_summary': {},
        'driver_exit_code': None, 'timed_out': False,
    }
    if gc_diagnostic:
        report['purpose'] = 'observational_gc_diagnostic_control'
        report['metadata'].update(
            diagnostic_only=True, shipping_agent_used=False,
            java_recipe='diagnostic_gc_logging_xmx512m_unchanged_heap_and_collector',
            java_launch='single_nonshipping_diagnostic_agent_normal_client_run',
            java_logging_argument=gc_control.LOGGING_ARGUMENT,
            logging_provenance='fixed_nonshipping_host_argument_private_owned_jvm_log',
            interpretation='logging_io_and_diagnostic_host_may_change_cpu_rss_and_latency_no_shipping_comparison')
        report['gc_selection_corroboration'] = gc_selection_status('not_checked')
    issues = set(fingerprint_issues)
    sampler = None
    child = None
    gc_selection = None
    start = time.perf_counter_ns()
    env = os.environ.copy()
    env.pop('CEDAR_RESOURCE_PHASE_PATH', None)
    # Inherited diagnostic selectors must never affect normal shipping runs.
    env.pop('CEDAR_GC_DIAGNOSTIC_AGENT_BIN', None)
    env.pop('CEDAR_GC_SELECTION_PATH', None)
    try:
        # Exclusive marker creation avoids reusing another run's readiness.
        with open(args.phase_file, 'xb'):
            pass
        env['CEDAR_RESOURCE_PHASE_PATH'] = str(Path(args.phase_file).absolute())
    except OSError:
        issues.add('phase_setup_failed')
    try:
        if gc_diagnostic:
            try:
                gc_selection = prepare_gc_selection(args.gc_owned_root, args.gc_selection)
            except (OSError, ValueError, TypeError):
                report['gc_selection_corroboration'] = gc_selection_status('selection_setup_rejected')
                raise ValueError() from None
            env['CEDAR_GC_DIAGNOSTIC_AGENT_BIN'] = str(Path(args.agent).absolute())
            env['CEDAR_GC_SELECTION_PATH'] = str(gc_selection[1])
        with open(args.transcript, 'ab', buffering=0) as transcript:
            child = subprocess.Popen([args.driver, test, '--exact',
                                      '--ignored', '--nocapture', '--test-threads=1'],
                                     stdout=transcript, stderr=subprocess.STDOUT, env=env)
            try:
                sampler = Sampler(backend, child.pid, {'headless_driver': args.driver,
                                                       'agent': args.agent, 'jvm': args.java},
                                  resolution_ns=resolution_ns, origin_ns=start)
            except ObservationError as error:
                issues.add(issue(error))
            while True:
                sweep = time.perf_counter_ns()
                events, latencies, event_issues = marker_events(args.phase_file, workload)
                issues.update(event_issues)
                report['phase_events'] = events
                report['latency_events'] = latencies
                if sampler is not None and len(report['samples']) < MAX_SAMPLES:
                    sample = sampler.sample(events[-1]['phase'] if events else 'starting')
                    after_events, after_latencies, after_issues = marker_events(args.phase_file, workload)
                    issues.update(after_issues)
                    sampler.validate_phase(sample, after_events[-1]['phase'] if after_events else 'starting',
                                           event_issues + after_issues)
                    report['phase_events'] = after_events
                    report['latency_events'] = after_latencies
                    end = time.perf_counter_ns()
                    sample['elapsed_ms'] = round((sweep - start) / 1_000_000, 3)
                    sample['sweep_ms'] = round((end - sweep) / 1_000_000, 3)
                    sample['sweep_start_elapsed_ns'] = sweep - start
                    sample['sweep_end_elapsed_ns'] = end - start
                    sample['observed_phase_after_sweep'] = after_events[-1]['phase'] if after_events else 'starting'
                    report['samples'].append(sample)
                elif sampler is not None:
                    issues.add('sample_limit')
                if child.poll() is not None:
                    break
                if (time.perf_counter_ns() - start) / 1e9 >= args.timeout_seconds:
                    report['timed_out'] = True
                    issues.add('driver_deadline')
                    # Only the directly owned driver is killed; never adopt a PID
                    # discovered by the observer as authority to terminate it.
                    child.kill()
                    try:
                        child.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        issues.add('driver_join_timeout')
                    break
                time.sleep(max(0, args.interval_ms / 1000 - (time.perf_counter_ns() - sweep) / 1e9))
            report['driver_exit_code'] = child.returncode
    except (OSError, ValueError, ObservationError):
        issues.add('observer_setup_or_io_failed')
        if child is not None:
            if child.poll() is None:
                # A failed observer still awaits the authorized workload, with
                # the same bounded deadline; observation is not its gate.
                try:
                    child.wait(timeout=max(0.1, args.timeout_seconds - (time.perf_counter_ns() - start) / 1e9))
                except subprocess.TimeoutExpired:
                    child.kill()
                    child.wait(timeout=5)
                    report['timed_out'] = True
            report['driver_exit_code'] = child.returncode
    finally:
        if sampler is not None:
            issues.update(sampler.issues)
            roles = {p.role for p in sampler.processes}
            if not {'headless_driver', 'agent', 'jvm'} <= roles:
                issues.add('required_role_not_observed')
            if gc_diagnostic:
                report['gc_selection_corroboration'] = sampler.corroborate_gc_selection(*gc_selection)
            sampler.close()
        elif gc_diagnostic and gc_selection is not None:
            report['gc_selection_corroboration'] = gc_selection_status('sampler_unavailable')
    if gc_diagnostic and not report['gc_selection_corroboration']['identity_corroborated']:
        issues.add('gc_selection_not_corroborated')
    if [event['phase'] for event in report['phase_events']] != list(phases):
        issues.add('phase_witness_incomplete')
    report['phase_summary'] = summary(report['samples'], phases)
    idle = report['phase_summary']['semantic_ready_idle']
    if idle[CPU_SUM]['valid_samples'] < 2:
        issues.add('idle_interval_incomplete')
    if workload in LONG_WORKLOADS:
        if [event['latency'] for event in report['latency_events']] != list(LATENCIES):
            issues.add('latency_witness_incomplete')
        report['idle_window_summary'] = idle_windows(report['samples'], report['phase_events'], args.interval_ms)
        if any(window['status'] != 'observed' for window in report['idle_window_summary'].values()):
            issues.add('idle_window_incomplete')
    report['issues'] = sorted(issues)
    report['status'] = 'complete' if not issues else 'incomplete'
    # JSON is generated entirely from fixed labels and typed numeric observations.
    try:
        Path(args.output).write_text(json.dumps(report, separators=(',', ':')) + '\n', encoding='utf-8')
    except OSError:
        print('Process-tree observation could not save its sanitized report.', file=sys.stderr)
    # Preserve the test result. Observer incompleteness is reported, not promoted
    # into a new product activation/acceptance threshold.
    return report['driver_exit_code'] if report['driver_exit_code'] is not None else 1


def numeric_fields(value, names):
    """Reconstruct numeric report fields; never copy input dictionaries/text."""
    if type(value) is not dict:
        raise ValueError()
    result = {}
    for name in names:
        number = value[name]
        if number is not None and (type(number) not in (int, float)
                                   or not 0 <= number <= 1e18 or not math.isfinite(number)):
            raise ValueError()
        result[name] = number
    return result


def projected_window(value):
    if (value['status'] not in ('observed', 'unknown')
            or value['requested_phase_ms'] != 30_000 or value['requested_window_ms'] != 10_000
            or value['window_basis'] != 'final_phase_stable_sweep_end_minus_ten_seconds_sampler_clock'
            or value['cpu']['interval_basis'] != 'independent_per_identity_whole_read_intervals_no_edge_interpolation'):
        raise ValueError()
    result = {'status': value['status'],
              'window_basis': 'final_phase_stable_sweep_end_minus_ten_seconds_sampler_clock',
              **numeric_fields(value, (
        'requested_phase_ms', 'observed_marker_duration_ms', 'requested_window_ms',
        'observed_end_elapsed_ns', 'first_sample_elapsed_ns', 'observed_sample_span_ns',
        'maximum_unsampled_gap_ns', 'trailing_phase_observation_gap_ns',
        'coverage_allowance_ns', 'stable_samples', 'unstable_samples'))}
    # A short/missing phase can have a negative requested start, which is only a
    # window target. It is intentionally not projected as an observed timestamp.
    result['rss_bytes'] = {}
    for scope in ('tree', *ROLES):
        source = value['rss_bytes']['tree'] if scope == 'tree' else value['rss_bytes']['by_role'][scope]
        result['rss_bytes'][scope] = numeric_fields(source, (
            'observed_samples', 'valid_samples', 'unknown_samples', 'median', 'observed_min', 'observed_max'))
    result['cpu'] = {}
    for scope in ('tree', *ROLES):
        source = value['cpu']['tree'] if scope == 'tree' else value['cpu']['by_role'][scope]
        result['cpu'][scope] = numeric_fields(source, (
            'independent_process_count', 'sum_of_independent_cpu_deltas_ns',
            'sum_of_independent_process_estimates_percent_one_core'))
    result['cpu']['interval_basis'] = 'independent_per_identity_whole_read_intervals_no_edge_interpolation'
    return result


def projected_trial(path, expected_trial):
    with open(path, 'rb') as source:
        data = source.read(64 * 1024 * 1024 + 1)
    if len(data) > 64 * 1024 * 1024:
        raise ValueError()
    report = json.loads(data, object_pairs_hook=unique_object)
    metadata = report['metadata']
    if (report['schema_version'] != 2 or report['platform'] != 'windows'
            or report['purpose'] != 'observational_process_tree_baseline'
            or report['status'] not in ('complete', 'incomplete')
            or metadata['workload'] != 'long_idle_baseline'
            or type(metadata['trial']) is not int or metadata['trial'] != expected_trial
            or metadata['test'] != LONG_TEST
            or metadata['driver_build'] != 'cargo_test_debug' or metadata['agent_build'] != 'release'
            or metadata['java_heap_limit_mib'] != 512
            or metadata['java_recipe'] != 'production_java_xmx512m_unchanged'
            or metadata['jdt_version'] != '1.61.0' or metadata['jdt_archive_sha256'] != JDT_SHA256
            or metadata['jdt_metadata_basis'] != 'pinned_expected_archive_verified_by_enclosing_acceptance_script'
            or metadata['input_fingerprint_basis'] != 'sha256_files_before_launch_not_loaded_image_attestation'
            or metadata['project_cache'] != 'fresh_generated_project_and_jdt_data'
            or metadata['os_file_cache'] != 'uncontrolled'
            or not re.fullmatch('[a-f0-9]{40,64}', metadata['source_commit'])):
        raise ValueError()
    measurement = report['measurement']
    if (measurement['clock'] != 'time.perf_counter_ns'
            or measurement['timeout_seconds'] != 270
            or measurement['cpu'] != 'per_process_read_midpoint_estimate_percent_of_one_logical_core'
            or measurement['cpu_aggregation'] != 'sum_of_process_estimates_not_same_window_tree_cpu'
            or type(measurement['interval_ms']) is not int or not 100 <= measurement['interval_ms'] <= 1000
            or type(metadata['logical_cpu_count']) is not int or not 1 <= metadata['logical_cpu_count'] <= 65536
            or type(metadata['python_version']) is not list or len(metadata['python_version']) != 3
            or any(type(part) is not int or not 0 <= part <= 1000 for part in metadata['python_version'])):
        raise ValueError()
    observation_settings = numeric_fields(measurement, ('interval_ms', 'timeout_seconds',
                                                         'clock_resolution_ns', 'cpu_counter_unit_ns'))
    if any(value is None or value <= 0 for value in observation_settings.values()):
        raise ValueError()
    observation_settings.update(logical_cpu_count=metadata['logical_cpu_count'],
                                python_version=list(metadata['python_version']), clock='time.perf_counter_ns')
    files = {}
    for role in ROLES[:3]:
        item = metadata['input_files'][role]
        if (not isinstance(item['sha256'], str) or not re.fullmatch('[a-f0-9]{64}', item['sha256'])
                or type(item['bytes']) is not int or not 0 < item['bytes'] <= 16 * 1024**3):
            raise ValueError()
        files[role] = {'sha256': item['sha256'], 'bytes': item['bytes']}
    latencies = []
    if type(report['latency_events']) is not list or len(report['latency_events']) > len(LATENCIES):
        raise ValueError()
    for index, event in enumerate(report['latency_events']):
        if (set(event) != {'latency', 'elapsed_ms', 'duration_ns'}
                or event['latency'] != LATENCIES[index]
                or type(event['elapsed_ms']) is not int or not 0 <= event['elapsed_ms'] <= 300_000
                or (latencies and event['elapsed_ms'] < latencies[-1]['elapsed_ms'])
                or type(event['duration_ns']) is not int or not 0 <= event['duration_ns'] <= 300_000_000_000):
            raise ValueError()
        latencies.append({'latency': LATENCIES[index], 'elapsed_ms': event['elapsed_ms'],
                          'duration_ns': event['duration_ns']})
    windows = {phase: projected_window(report['idle_window_summary'][phase]) for phase in IDLE_PHASES}
    if any(window['coverage_allowance_ns'] != 2 * measurement['interval_ms'] * 1_000_000
           for window in windows.values()):
        raise ValueError()
    observation_complete = (report['status'] == 'complete' and len(latencies) == len(LATENCIES)
                            and report['driver_exit_code'] == 0 and report['timed_out'] is False
                            and all(window['status'] == 'observed'
                                    and window['rss_bytes']['tree']['median'] is not None
                                    and window['cpu']['tree']['sum_of_independent_process_estimates_percent_one_core'] is not None
                                    for window in windows.values()))
    return {'trial': expected_trial, 'source_commit': metadata['source_commit'],
            'input_files': files, 'observation_complete': observation_complete,
            'observation_settings': observation_settings,
            'idle_window_summary': windows, 'latency_events': latencies}


def compare_reports(first, second):
    result = {'schema_version': 1, 'purpose': 'two_unchanged_long_idle_observations',
              'status': 'not_comparable', 'same_input_files_and_source': False,
              'workload': 'long_idle_baseline', 'test': LONG_TEST,
              'java_heap_limit_mib': 512, 'jdt_version': '1.61.0', 'jdt_archive_sha256': JDT_SHA256,
              'jdt_metadata_basis': 'pinned_expected_archive_verified_by_enclosing_acceptance_script',
              'input_fingerprint_basis': 'sha256_files_before_launch_not_loaded_image_attestation',
              'project_cache': 'fresh_generated_project_and_jdt_data_each_trial',
              'os_file_cache': 'uncontrolled_second_trial_may_benefit',
              'interpretation': 'descriptive_observations_no_optimization_or_regression_claim',
              'run_metadata_artifact': 'cedar-windows-java-acceptance.txt', 'trials': []}
    try:
        trials = [projected_trial(first, 1), projected_trial(second, 2)]
        if (trials[0]['source_commit'] != trials[1]['source_commit']
                or trials[0]['input_files'] != trials[1]['input_files']
                or trials[0]['observation_settings'] != trials[1]['observation_settings']):
            return result
        result['same_input_files_and_source'] = True
        result['trials'] = trials
        result['status'] = ('observed' if all(trial['observation_complete'] for trial in trials)
                            else 'observation_incomplete')
    except (OSError, ValueError, KeyError, TypeError, RecursionError, UnicodeError):
        pass
    return result


class PrivateArgumentParser(argparse.ArgumentParser):
    def error(self, message):
        # argparse's default invalid-argument errors echo private input paths.
        print('Process-tree observation arguments are invalid.', file=sys.stderr)
        raise SystemExit(2)


def main():
    if len(sys.argv) > 1 and sys.argv[1] == 'compare':
        parser = PrivateArgumentParser(description='Compare two fixed unchanged long idle observations.')
        for name in ('trial-1', 'trial-2', 'output'):
            parser.add_argument('--' + name, required=True)
        args = parser.parse_args(sys.argv[2:])
        report = compare_reports(args.trial_1, args.trial_2)
        try:
            Path(args.output).write_text(json.dumps(report, separators=(',', ':'), allow_nan=False) + '\n', encoding='utf-8')
        except OSError:
            print('Process-tree comparison could not save its sanitized report.', file=sys.stderr)
            return 1
        return 1 if report['status'] == 'not_comparable' else 0
    parser = PrivateArgumentParser(description=__doc__)
    for name in ('driver', 'agent', 'java', 'phase-file', 'transcript', 'output', 'source-commit'):
        parser.add_argument('--' + name, required=True)
    parser.add_argument('--interval-ms', type=int, default=200, choices=range(100, 1001))
    parser.add_argument('--timeout-seconds', type=int, default=270, choices=range(5, 301))
    parser.add_argument('--workload', choices=WORKLOADS, default='normal_acceptance')
    parser.add_argument('--trial', type=int, choices=(1, 2))
    parser.add_argument('--gc-owned-root')
    parser.add_argument('--gc-selection')
    args = parser.parse_args()
    if not re.fullmatch('[a-f0-9]{40,64}', args.source_commit):
        parser.error('source commit must be a full lowercase hexadecimal identifier')
    if (args.workload == 'long_idle_baseline') != (args.trial is not None):
        parser.error('only a long idle baseline observation has a fixed trial number')
    if args.workload in LONG_WORKLOADS and args.timeout_seconds != 270:
        parser.error('the long idle observation has a fixed 270-second observer deadline')
    if args.workload == 'gc_diagnostic_control':
        if args.gc_owned_root is None or args.gc_selection is None or sys.platform != 'win32':
            parser.error('the Windows GC control requires its owned root and private selection path')
    elif args.gc_owned_root is not None or args.gc_selection is not None:
        parser.error('GC selection arguments are exclusive to the diagnostic control')
    if sys.platform not in ('win32', 'linux'):
        parser.error('only native Windows and Linux observation are supported')
    try:
        return run(args)
    except (OSError, ValueError, subprocess.SubprocessError):
        # Never let Python render paths or child commands in a public traceback.
        print('Process-tree observation could not save its sanitized report.', file=sys.stderr)
        return 1


if __name__ == '__main__':
    raise SystemExit(main())
