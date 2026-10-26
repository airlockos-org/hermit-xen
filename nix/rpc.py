"""Bounded argv execution over a private virtio serial channel. Policy lives in Rust."""
import json
import os
import subprocess
import traceback
import time

os.makedirs('/lab', exist_ok=True)
print('LAB_RPC_READY', flush=True)
with open('/dev/virtio-ports/lab.control', 'r+b', buffering=0) as channel:
    buffer = b''
    while True:
        chunk = channel.read(65536)
        if not chunk:
            time.sleep(0.05)
            continue
        buffer += chunk
        while b'\n' in buffer:
            line, buffer = buffer.split(b'\n', 1)
            req = {}
            try:
                req = json.loads(line)
                timeout = min(max(float(req.get('timeout', 10)), 1), 120)
                # A process group makes timeout apply to descendants as well.
                proc = subprocess.Popen(req['argv'], stdout=subprocess.PIPE,
                                        stderr=subprocess.PIPE, start_new_session=True)
                try:
                    out, err = proc.communicate(timeout=timeout)
                    result = dict(code=proc.returncode, stdout=out.decode(errors='replace'),
                                  stderr=err.decode(errors='replace'), timed_out=False)
                except subprocess.TimeoutExpired:
                    import signal
                    os.killpg(proc.pid, signal.SIGKILL)
                    out, err = proc.communicate()
                    result = dict(code=-1, stdout=out.decode(errors='replace'),
                                  stderr=err.decode(errors='replace'), timed_out=True)
            except Exception:
                result = dict(code=-1, stdout='', stderr=traceback.format_exc(), timed_out=False)
            result['id'] = req.get('id')
            payload = memoryview((json.dumps(result) + '\n').encode())
            while payload:
                payload = payload[channel.write(payload):]
