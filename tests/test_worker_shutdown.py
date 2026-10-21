"""The Rust/SSH parent keeps stdin open until after the worker exits."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


class WorkerShutdownTests(unittest.TestCase):
    def environment(self):
        return {**os.environ, 'PYTHONPATH': str(ROOT / 'src-tauri/resources'),
                'ML_WATCH_STDIN': '1', 'PYTHONUTF8': '1',
                'OPENBLAS_NUM_THREADS': '2', 'OMP_NUM_THREADS': '2'}

    def test_normal_and_error_exit_while_parent_holds_stdin_open(self):
        with tempfile.TemporaryDirectory() as temp:
            request = Path(temp) / 'request.json'
            for action, expected in [('check', 0), ('missing', 2)]:
                with self.subTest(action=action):
                    request.write_text(json.dumps({'action': action}), encoding='utf-8')
                    with subprocess.Popen([sys.executable, '-m', 'analysis_worker', str(request)],
                                          stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                          stderr=subprocess.PIPE, env=self.environment()) as child:
                        try:
                            code = child.wait(timeout=30)
                        except subprocess.TimeoutExpired:
                            child.kill(); child.wait()
                            self.fail('worker did not exit while parent held stdin open')
                        output = child.stdout.read().decode('utf-8')
                        error = child.stderr.read().decode('utf-8')
                        self.assertEqual(code, expected, error)
                        self.assertNotIn('Fatal Python error', error)
                        self.assertEqual(json.loads(output.splitlines()[-1])['event'],
                                         'result' if expected == 0 else 'error')

    def test_parent_disconnect_still_terminates_worker(self):
        program = "from analysis_worker.__main__ import watch_parent; t=watch_parent(); print('ready',flush=True); t.join()"
        with subprocess.Popen([sys.executable, '-c', program], stdin=subprocess.PIPE,
                              stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                              env=self.environment()) as child:
            try:
                self.assertEqual(child.stdout.readline().decode().strip(), 'ready')
                child.stdin.write(b'ping'); child.stdin.flush()
                self.assertIsNone(child.poll())
                child.stdin.close()
                self.assertEqual(child.wait(timeout=15), 130)
                self.assertNotIn(b'Fatal Python error', child.stderr.read())
            finally:
                if child.poll() is None:
                    child.kill(); child.wait()


if __name__ == '__main__':
    unittest.main()
