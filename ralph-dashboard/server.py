#!/usr/bin/env python3
"""
Ralph Mission Control Server
A simple server to serve the dashboard and stream log updates.
"""

import http.server
import json
import os
import socketserver
import sys
import time
from pathlib import Path
from urllib.parse import urlparse, parse_qs

PORT = 8888
LOG_DIR = Path(__file__).parent.parent / "ralph-logs"
CURRENT_LOG = LOG_DIR / "current-run.jsonl"

class MissionControlHandler(http.server.SimpleHTTPRequestHandler):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=str(Path(__file__).parent), **kwargs)

    def do_GET(self):
        parsed = urlparse(self.path)

        if parsed.path == "/api/log":
            self.serve_log()
        elif parsed.path == "/api/log/stream":
            self.stream_log()
        elif parsed.path == "/api/logs":
            self.list_logs()
        elif parsed.path.startswith("/api/log/"):
            log_name = parsed.path.split("/")[-1]
            self.serve_specific_log(log_name)
        else:
            super().do_GET()

    def serve_log(self):
        """Serve the current log file as JSON array"""
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Cache-Control", "no-cache")
        self.end_headers()

        events = []
        if CURRENT_LOG.exists():
            with open(CURRENT_LOG, "r") as f:
                for line in f:
                    line = line.strip()
                    if line:
                        try:
                            events.append(json.loads(line))
                        except json.JSONDecodeError:
                            pass

        self.wfile.write(json.dumps(events).encode())

    def stream_log(self):
        """Stream log updates using Server-Sent Events"""
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Cache-Control", "no-cache")
        self.send_header("Connection", "keep-alive")
        self.end_headers()

        # Parse query params for offset
        parsed = urlparse(self.path)
        params = parse_qs(parsed.query)
        offset = int(params.get("offset", [0])[0])

        try:
            last_size = 0
            line_count = 0

            while True:
                if CURRENT_LOG.exists():
                    current_size = CURRENT_LOG.stat().st_size

                    if current_size != last_size:
                        with open(CURRENT_LOG, "r") as f:
                            lines = f.readlines()

                        # Send new lines
                        for i, line in enumerate(lines):
                            if i >= offset:
                                line = line.strip()
                                if line:
                                    try:
                                        event = json.loads(line)
                                        data = json.dumps({"index": i, "event": event})
                                        self.wfile.write(f"data: {data}\n\n".encode())
                                        self.wfile.flush()
                                        offset = i + 1
                                    except json.JSONDecodeError:
                                        pass

                        last_size = current_size

                # Send keepalive
                self.wfile.write(": keepalive\n\n".encode())
                self.wfile.flush()
                time.sleep(0.5)

        except (BrokenPipeError, ConnectionResetError):
            pass

    def list_logs(self):
        """List all available log files"""
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Access-Control-Allow-Origin", "*")
        self.end_headers()

        logs = []
        if LOG_DIR.exists():
            for f in sorted(LOG_DIR.glob("ralph_*.jsonl"), reverse=True):
                stats = f.stat()
                logs.append({
                    "name": f.name,
                    "size": stats.st_size,
                    "modified": stats.st_mtime
                })

        self.wfile.write(json.dumps(logs).encode())

    def serve_specific_log(self, log_name):
        """Serve a specific archived log file"""
        log_path = LOG_DIR / log_name

        if not log_path.exists() or not log_name.endswith(".jsonl"):
            self.send_error(404, "Log not found")
            return

        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Access-Control-Allow-Origin", "*")
        self.end_headers()

        events = []
        with open(log_path, "r") as f:
            for line in f:
                line = line.strip()
                if line:
                    try:
                        events.append(json.loads(line))
                    except json.JSONDecodeError:
                        pass

        self.wfile.write(json.dumps(events).encode())


def main():
    os.chdir(Path(__file__).parent)

    with socketserver.TCPServer(("", PORT), MissionControlHandler) as httpd:
        print(f"\n{'='*60}")
        print(f"  RALPH MISSION CONTROL")
        print(f"{'='*60}")
        print(f"  Dashboard: http://localhost:{PORT}")
        print(f"  Log API:   http://localhost:{PORT}/api/log")
        print(f"  Stream:    http://localhost:{PORT}/api/log/stream")
        print(f"{'='*60}")
        print(f"  Press Ctrl+C to stop\n")

        try:
            httpd.serve_forever()
        except KeyboardInterrupt:
            print("\nShutting down...")
            sys.exit(0)


if __name__ == "__main__":
    main()
