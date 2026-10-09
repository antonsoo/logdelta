#!/usr/bin/env python3
"""Capture three controlled HTTP runs. Standard library only; loopback traffic only.

The failed server deliberately returns 503 for requests 7 and 15, then exits 1.
These are real server-emitted records from an injected fault, not production evidence.
Run: python examples/capture_http.py [--output-directory /tmp/http-example]
Add --scenario mixed-routes to interleave a maintenance route that always returns 503.
Add --scenario rate-change for 1,000 checkout and 1,000 maintenance responses per run,
with checkout failures rising from 10/12 in the baselines to 200 in the target.
"""

import argparse
import datetime
import http.client
from http.server import BaseHTTPRequestHandler, HTTPServer
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import time


def response_status(profile, scenario, route, checkout_index):
    if route == "/maintenance":
        return 503
    if scenario == "rate-change":
        return 503 if checkout_index < {"good": 10, "good-2": 12, "failed": 200}[profile] else 200
    return 503 if profile == "failed" and checkout_index in (6, 14) else 200


def request_schedule(scenario):
    routes = ["/checkout"] if scenario == "single-route" else ["/checkout", "/maintenance"]
    return [(i, route) for i in range(1000 if scenario == "rate-change" else 20) for route in routes]


def serve(profile, port_file, scenario):
    class Handler(BaseHTTPRequestHandler):
        count = 0
        checkout_count = 0
        failures = 0

        def log_message(self, *_args):
            pass

        def do_GET(self):
            started = time.perf_counter_ns()
            type(self).count += 1
            if self.path == "/checkout":
                type(self).checkout_count += 1
            status = response_status(profile, scenario, self.path, self.checkout_count - 1)
            failed = self.path == "/checkout" and profile == "failed" and status == 503
            type(self).failures += int(failed)
            body = json.dumps({"ok": status == 200}).encode()
            self.send_response(status)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            self.wfile.flush()
            print(json.dumps({
                "ts": datetime.datetime.now(datetime.timezone.utc).isoformat(),
                "event": "request_complete", "route": self.path,
                "http": {"status": status},
                "elapsed_ms": round((time.perf_counter_ns() - started) / 1_000_000, 3),
                "request_id": 9007199254740992 + self.count,
            }, separators=(",", ":")), flush=True)

    with HTTPServer(("127.0.0.1", 0), Handler) as server:
        server.timeout = 10
        print("checkout service ready", flush=True)
        Path(port_file).write_text(str(server.server_port))
        for _ in request_schedule(scenario):
            previous = Handler.count
            server.handle_request()
            if Handler.count == previous:
                raise TimeoutError("client did not send its next request")
    exit_code = int(Handler.failures > 0)
    print(json.dumps({"event": "worker_exit", "exit_code": exit_code}, separators=(",", ":")), flush=True)
    return exit_code


def capture(profile, scenario):
    # A large capture must not fill an unread stdout pipe and stall the server mid-request.
    with tempfile.TemporaryDirectory(prefix="logdelta-http-") as directory, tempfile.TemporaryFile(mode="w+", encoding="utf-8") as output:
        port_file = Path(directory) / "port"
        process = subprocess.Popen(
            [sys.executable, __file__, "--serve", profile, "--port-file", str(port_file), "--scenario", scenario],
            stdout=output, stderr=subprocess.PIPE, text=True,
        )
        try:
            deadline = time.monotonic() + 10
            while not port_file.exists() or not port_file.read_text():
                if process.poll() is not None or time.monotonic() > deadline:
                    raise RuntimeError("HTTP fixture server did not start")
                time.sleep(0.01)
            statuses = []
            schedule = request_schedule(scenario)
            for _, route in schedule:
                connection = http.client.HTTPConnection("127.0.0.1", int(port_file.read_text()), timeout=5)
                try:
                    connection.request("GET", route)
                    response = connection.getresponse()
                    statuses.append(response.status)
                    assert json.loads(response.read())["ok"] == (response.status == 200)
                finally:
                    connection.close()
            _, stderr = process.communicate(timeout=10)
            output.seek(0)
            stdout = output.read()
            expected = [response_status(profile, scenario, route, i) for i, route in schedule]
            assert statuses == expected, statuses
            if scenario == "rate-change":
                observed_failures = sum(status == 503 and route == "/checkout" for (_, route), status in zip(schedule, statuses))
                assert observed_failures == {"good": 10, "good-2": 12, "failed": 200}[profile]
            assert process.returncode == int(profile == "failed"), (process.returncode, stderr)
            records = stdout.splitlines()
            assert len(records) == len(schedule) + 2, records
            assert [json.loads(line)["http"]["status"] for line in records[1:-1]] == statuses
            assert [json.loads(line)["route"] for line in records[1:-1]] == [route for _, route in schedule]
            assert json.loads(records[-1])["exit_code"] == process.returncode
            return stdout, {
                "process_exit": process.returncode,
                "responses": [{"route": route, "status": status} for (_, route), status in zip(schedule, statuses)],
            }
        finally:
            if process.poll() is None:
                process.kill()
            process.communicate()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-directory", type=Path)
    parser.add_argument("--serve", choices=["good", "good-2", "failed"], help=argparse.SUPPRESS)
    parser.add_argument("--port-file", help=argparse.SUPPRESS)
    parser.add_argument("--scenario", choices=["single-route", "mixed-routes", "rate-change"], default="single-route")
    args = parser.parse_args()
    if args.serve:
        return serve(args.serve, args.port_file, args.scenario)
    prefix = {"single-route": "http", "mixed-routes": "http-routes", "rate-change": "http-rate"}[args.scenario]
    profiles = [("good", "good"), ("good-2", "good-2" if args.scenario == "rate-change" else "good"), ("failed", "failed")]
    captures = [(f"{prefix}-{name}.log", capture(profile, args.scenario)) for name, profile in profiles]
    if args.output_directory is None:
        args.output_directory = Path(__file__).resolve().parent
        if args.scenario == "rate-change":
            args.output_directory /= "http-rates"
    args.output_directory.mkdir(parents=True, exist_ok=True)
    evidence = {"scenario": args.scenario, "provenance": "Controlled loopback HTTP requests; faults injected, not a production incident.", "runs": {}}
    for name, (content, observed) in captures:
        (args.output_directory / name).write_text(content)
        evidence["runs"][name] = observed
        print(f"{name}: {len(content.splitlines())} lines; HTTP responses and process exit checked")
    (args.output_directory / f"{prefix}-observed.json").write_text(json.dumps(evidence, indent=2) + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
