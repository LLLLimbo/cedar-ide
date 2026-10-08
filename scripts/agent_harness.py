#!/usr/bin/env python3
"""Bounded Linux/POSIX stdio harness for synthetic cedar-agent integration tests."""
import json
import os
import selectors
import signal
import subprocess
import time

MAX_FRAME = 8 * 1024 * 1024


class AgentError(RuntimeError):
    def __init__(self, kind, detail):
        self.kind, self.detail = kind, detail
        super().__init__(f'{kind}: {detail}')


class Agent:
    def __init__(self, binary, root, *, allow_run=True):
        arguments = [str(binary), '--root', str(root)]
        if allow_run:
            arguments.append('--allow-run')
        self.process = subprocess.Popen(arguments,
                                        stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                        stderr=subprocess.PIPE, start_new_session=True)
        self.selector = selectors.DefaultSelector()
        self.selector.register(self.process.stdout, selectors.EVENT_READ, 'stdout')
        self.selector.register(self.process.stderr, selectors.EVENT_READ, 'stderr')
        self.buffer = bytearray()
        self.stderr = bytearray()
        self.counter = 0

    def call(self, operation, *, timeout=20, ok=True, **fields):
        self.counter += 1
        frame = json.dumps({'id': self.counter, 'op': {'type': operation, **fields}}).encode() + b'\n'
        assert len(frame) <= MAX_FRAME
        self.process.stdin.write(frame)
        self.process.stdin.flush()
        deadline = time.monotonic() + timeout
        while b'\n' not in self.buffer:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise TimeoutError(f'{operation}: no agent response within {timeout}s')
            for key, _ in self.selector.select(remaining):
                chunk = os.read(key.fileobj.fileno(), 65536)
                if not chunk:
                    self.selector.unregister(key.fileobj)
                    if key.data == 'stdout':
                        raise RuntimeError(f'Agent EOF during {operation}: {self.stderr.decode(errors="replace")}')
                elif key.data == 'stdout':
                    self.buffer.extend(chunk)
                    if len(self.buffer) > MAX_FRAME:
                        raise RuntimeError('Agent response exceeded wire limit')
                else:
                    self.stderr.extend(chunk)
                    del self.stderr[:-256 * 1024]
        raw, _, remaining = self.buffer.partition(b'\n')
        self.buffer = bytearray(remaining)
        response = json.loads(raw)
        assert response['id'] == self.counter, response
        result = response['result']
        if ('Ok' in result) != ok:
            raise AgentError(operation, result)
        return result.get('Ok', result.get('Err'))

    def close(self):
        self.process.stdin.close()
        try:
            self.process.wait(timeout=8)
        except subprocess.TimeoutExpired:
            # Owned, unreaped child is the session leader. No other process/PID lookup.
            os.killpg(self.process.pid, signal.SIGKILL)
            self.process.wait(timeout=5)
        self.selector.close()
        self.process.stdout.close()
        self.process.stderr.close()
