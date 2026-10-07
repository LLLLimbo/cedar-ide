"""Owned, synthetic integration-test code. Never use for user programs."""
import os
import signal
import sys
import time

# OS-default SIGALRM terminates even while paused in the debugger. Test-only
# containment: production debuggees do not have this bounded lifetime.
signal.alarm(15)
with open(sys.argv[1], "w", encoding="utf-8") as marker:
    marker.write(str(os.getpid()))
answer = 41
answer += 1  # CEDAR_BREAKPOINT
print(f"CEDAR_ANSWER={answer}", flush=True)
time.sleep(0.05)
