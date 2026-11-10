"""Explicit opt-in: real Steam backend; never run by the default offline suite."""

import json
import os
from pathlib import Path
import socket
import signal
import subprocess
import sys
import tempfile
import unittest


@unittest.skipUnless(os.environ.get("GOLDSRC_VERIFY_STEAM") == "1", "requires explicit live Steam opt-in and local SDK/runtime")
class LiveSdkTests(unittest.TestCase):
    def test_rust_logon_real_identity_and_rejection_of_synthetic_ticket(self):
        root = Path(__file__).resolve().parents[2]
        local = root / "fixtures/local"
        with tempfile.TemporaryDirectory(prefix="steam-live-test-", dir=local) as work:
            trace = Path(work) / "trace.jsonl"
            log = Path(work) / "process.log"
            with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as reservation:
                reservation.bind(("127.0.0.1", 0))
                port = reservation.getsockname()[1]
            command = [sys.executable, str(root / "poc/steam-auth-probe/probe.py"),
                       "--run-server", str(root / "poc/steam-server-query/target/debug/steam-server-query-poc"),
                       "--listen", f"127.0.0.1:{port}", "--seconds", "15", "--trace", str(trace)]
            with log.open("w+") as output:
                process = subprocess.Popen(command, stdout=output, stderr=subprocess.STDOUT, start_new_session=True)
                try:
                    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as client:
                        client.settimeout(0.2)
                        response = None
                        for _ in range(60):
                            self.assertIsNone(process.poll(), "native diagnostic exited before challenge")
                            client.sendto(b"\xff\xff\xff\xffgetchallenge steam\n", ("127.0.0.1", port))
                            try:
                                response, _ = client.recvfrom(1400)
                                break
                            except socket.timeout:
                                continue
                        self.assertIsNotNone(response, "no game challenge after real Steam logon")
                        self.assertTrue(response.startswith(b"\xff\xff\xff\xffA00000000 "))
                        fields = response[4:].decode("ascii").split()
                        self.assertGreater(int(fields[3]), 0, "must advertise real server Steam ID")
                        request = b"\xff\xff\xff\xffconnect 48 " + fields[1].encode("ascii")
                        request += b' "\\prot\\3" "\\name\\Synthetic\\password\\synthetic-secret"\nnot-a-real-steam-ticket'
                        client.sendto(request, ("127.0.0.1", port))
                        client.settimeout(5)
                        rejection, _ = client.recvfrom(1400)
                        self.assertTrue(rejection.startswith(b"\xff\xff\xff\xff9"))
                    self.assertEqual(process.wait(timeout=35), 0)
                    contents = trace.read_text()
                    records = [json.loads(line) for line in contents.splitlines()]
                    outcomes = [record["outcome"] for record in records if record.get("record") == "connection_outcome"]
                    self.assertIn("steam_auth_rejected_basic", outcomes)
                    self.assertNotIn("steam_auth_approved", outcomes)
                    self.assertNotIn(b"not-a-real-steam-ticket".hex(), contents)
                    self.assertNotIn(b"synthetic-secret".hex(), contents)
                    print("Live SDK verified: Rust server logon, real challenge identity, invalid-ticket rejection, trace redaction, timed cleanup")
                finally:
                    if process.poll() is None:
                        os.killpg(process.pid, signal.SIGTERM)
                        try:
                            process.wait(timeout=5)
                        except subprocess.TimeoutExpired:
                            os.killpg(process.pid, signal.SIGKILL)
                            process.wait(timeout=5)


if __name__ == "__main__":
    unittest.main()
