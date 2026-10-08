#!/usr/bin/env python3
"""Bounded observational sampling of one owned, prebuilt headless acceptance run.

No process names, PIDs, paths, command lines, environment or exception strings
are serialized. Child stdout/stderr stay in the caller's private transcript.
The sampler never decides whether resource consumption passes acceptance.
"""
import argparse
import ctypes
from dataclasses import dataclass
import json
import math
import os
from pathlib import Path
import re
import subprocess
import sys
import time

ROLES = ('headless_driver', 'agent', 'jvm', 'other_descendant')
PHASES = ('starting', 'java_initialized', 'semantic_ready_idle', 'query_workload',
          'cleanup', 'complete')
LIVE_METRICS = ('rss_bytes', 'threads', 'handles')
CPU_SUM = 'cpu_percent_one_core_sum_of_process_estimates'
METRICS = (*LIVE_METRICS, CPU_SUM)
CPU_ISSUES = frozenset(('cpu_timing_invalid', 'cpu_counter_invalid'))
CPU_INTERVAL_FIELDS = ('cpu_delta_ns', 'elapsed_ns', 'elapsed_min_ns', 'elapsed_max_ns',
                       'previous_read_span_ns', 'current_read_span_ns',
                       'percent_lower_from_timing', 'percent_upper_from_timing')
MAX_PROCESSES = 64
MAX_SAMPLES = 1600
MAX_SYSTEM_PROCESSES = 32768
MAX_MARKER_BYTES = 8192
PRODUCTION_TEST = ('language_ui::real_java_tests::acceptance::windows::production::'
                   'real_windows_normal_agent_java_editor_acceptance')
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


def phase_events(path):
    """Allowlist typed records; never retain arbitrary marker file contents."""
    try:
        with open(path, 'rb') as source:
            data = source.read(MAX_MARKER_BYTES + 1)
    except FileNotFoundError:
        return [], []
    except OSError:
        return [], ['phase_unavailable']
    if len(data) > MAX_MARKER_BYTES:
        return [], ['phase_limit']
    events = []
    try:
        # A partial final write is retried next sample.
        for line in data.splitlines(keepends=True):
            if not line.endswith(b'\n'):
                continue
            value = json.loads(line)
            if (not isinstance(value, dict) or set(value) != {'phase', 'elapsed_ms'}
                    or value['phase'] not in PHASES
                    or type(value['elapsed_ms']) is not int
                    or not 0 <= value['elapsed_ms'] <= 300_000
                    or len(events) >= len(PHASES)
                    or (events and value['elapsed_ms'] < events[-1]['elapsed_ms'])
                    or value['phase'] != PHASES[len(events)]):
                raise ValueError()
            events.append(value)
    except (ValueError, RecursionError, UnicodeError):
        return [], ['phase_invalid']
    return events, []


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
    def __init__(self, backend, root_pid, roles, resolution_ns=None):
        self.backend = backend
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
                percent, interval, cpu_issue = cpu_interval(process, values, phase, self.resolution_ns)
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


def summary(samples):
    result = {}
    for phase in PHASES:
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


def run(args):
    backend = Windows() if sys.platform == 'win32' else Linux()
    resolution_ns = math.ceil(time.get_clock_info('perf_counter').resolution * 1e9)
    report = {
        'schema_version': 2, 'purpose': 'observational_process_tree_baseline',
        'status': 'incomplete', 'acceptance_evaluated': False,
        'platform': 'windows' if sys.platform == 'win32' else 'linux',
        'metadata': {'source_commit': args.source_commit, 'driver_build': 'cargo_test_debug',
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
                        'idle': 'two_seconds_after_exact_initial_diagnostics_background_work_may_continue',
                        'excludes': ['sampler', 'cargo_and_compiler', 'gui_rendering', 'ssh',
                                     'earlier_acceptance_runs', 'system_services']},
        'issues': [], 'phase_events': [], 'samples': [], 'phase_summary': {},
        'driver_exit_code': None, 'timed_out': False,
    }
    issues = set()
    sampler = None
    child = None
    start = time.perf_counter_ns()
    env = os.environ.copy()
    env.pop('CEDAR_RESOURCE_PHASE_PATH', None)
    try:
        # Exclusive marker creation avoids reusing another run's readiness.
        with open(args.phase_file, 'xb'):
            pass
        env['CEDAR_RESOURCE_PHASE_PATH'] = str(Path(args.phase_file).absolute())
    except OSError:
        issues.add('phase_setup_failed')
    try:
        with open(args.transcript, 'ab', buffering=0) as transcript:
            child = subprocess.Popen([args.driver, PRODUCTION_TEST, '--exact',
                                      '--ignored', '--nocapture', '--test-threads=1'],
                                     stdout=transcript, stderr=subprocess.STDOUT, env=env)
            try:
                sampler = Sampler(backend, child.pid, {'headless_driver': args.driver,
                                                       'agent': args.agent, 'jvm': args.java},
                                  resolution_ns=resolution_ns)
            except ObservationError as error:
                issues.add(issue(error))
            while True:
                sweep = time.perf_counter_ns()
                events, event_issues = phase_events(args.phase_file)
                issues.update(event_issues)
                report['phase_events'] = events
                if sampler is not None and len(report['samples']) < MAX_SAMPLES:
                    sample = sampler.sample(events[-1]['phase'] if events else 'starting')
                    after_events, after_issues = phase_events(args.phase_file)
                    issues.update(after_issues)
                    sampler.validate_phase(sample, after_events[-1]['phase'] if after_events else 'starting',
                                           event_issues + after_issues)
                    report['phase_events'] = after_events
                    sample['elapsed_ms'] = round((sweep - start) / 1_000_000, 3)
                    sample['sweep_ms'] = round((time.perf_counter_ns() - sweep) / 1_000_000, 3)
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
            sampler.close()
    if [event['phase'] for event in report['phase_events']] != list(PHASES):
        issues.add('phase_witness_incomplete')
    report['phase_summary'] = summary(report['samples'])
    idle = report['phase_summary']['semantic_ready_idle']
    if idle[CPU_SUM]['valid_samples'] < 2:
        issues.add('idle_interval_incomplete')
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


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('driver', 'agent', 'java', 'phase-file', 'transcript', 'output', 'source-commit'):
        parser.add_argument('--' + name, required=True)
    parser.add_argument('--interval-ms', type=int, default=200, choices=range(100, 1001))
    parser.add_argument('--timeout-seconds', type=int, default=270, choices=range(5, 301))
    args = parser.parse_args()
    if not re.fullmatch('[a-f0-9]{40,64}', args.source_commit):
        parser.error('source commit must be a full lowercase hexadecimal identifier')
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
