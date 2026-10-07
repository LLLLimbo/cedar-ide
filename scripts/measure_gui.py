#!/usr/bin/env python3
"""Interactive Linux GUI measurement wrapper: close Cedar normally to record child rusage.
This includes the app process and collected child usage, not GPU-service memory.
"""
import json, os, pathlib, platform, resource, subprocess, sys, time
exe=str(pathlib.Path(sys.argv[1]).resolve());out=pathlib.Path(sys.argv[2]);started=time.monotonic()
p=subprocess.Popen([exe]);status=p.wait();usage=resource.getrusage(resource.RUSAGE_CHILDREN)
out.write_text(json.dumps({"platform":platform.platform(),"executable":exe,"elapsed_seconds":round(time.monotonic()-started,3),"exit_code":status,"max_rss_kib":usage.ru_maxrss,"user_cpu_seconds":usage.ru_utime,"system_cpu_seconds":usage.ru_stime,"note":"interactive sample, no IDEA comparison; GPU service memory excluded"},indent=2)+"\n")
sys.exit(status)
