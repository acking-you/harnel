#!/usr/bin/env python3
"""Exercise the real CLI against local model and ACP transports."""
import json
import os
import pathlib
import selectors
import signal
import socket
import subprocess
import sys
import tempfile
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

binary = str(pathlib.Path(sys.argv[1]).resolve())


class Model(BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_POST(self):
        self.rfile.read(int(self.headers["Content-Length"]))
        events = [
            {"type": "response.output_text.delta", "item_id": "answer", "output_index": 0,
             "content_index": 0, "delta": "CLI_SMOKE_OK"},
            {"type": "response.completed", "response": {"status": "completed",
             "usage": {"input_tokens": 3, "output_tokens": 2}}},
        ]
        body = ("".join(f"data: {json.dumps(event)}\n\n" for event in events) + "data: [DONE]\n\n").encode()
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


def request(write, read, number, method, params):
    write.write((json.dumps({"jsonrpc": "2.0", "id": number, "method": method, "params": params}) + "\n").encode())
    write.flush()
    while True:
        selector = selectors.DefaultSelector()
        try:
            selector.register(read, selectors.EVENT_READ)
            if not selector.select(15):
                raise RuntimeError(f"timeout waiting for {method}")
        finally:
            selector.close()
        line = read.readline()
        if not line:
            raise RuntimeError(f"EOF waiting for {method}")
        value = json.loads(line)
        if value.get("id") == number:
            assert "error" not in value, value
            return value["result"]


with tempfile.TemporaryDirectory() as root:
    server = ThreadingHTTPServer(("127.0.0.1", 0), Model)
    thread = threading.Thread(target=server.serve_forever)
    thread.start()
    try:
        result = subprocess.run([binary, "run", "Reply with CLI_SMOKE_OK", "--workspace", root,
            "--model", "openai/gpt-5", "--base-url", f"http://127.0.0.1:{server.server_port}/v1",
            "--api-key-env", "HARNEL_SMOKE_KEY", "--no-native-tools"],
            env={**os.environ, "HARNEL_SMOKE_KEY": "synthetic-fixture-key"},
            text=True, capture_output=True, timeout=30)
        assert result.returncode == 0, result.stderr
        assert result.stdout.strip() == "CLI_SMOKE_OK", result.stdout
        assert not result.stderr, result.stderr
    finally:
        server.shutdown()
        server.server_close()
        thread.join()

    process = subprocess.Popen([binary, "acp", "--stdio", "--listen", "127.0.0.1:0", "--workspace", root],
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, bufsize=0)
    try:
        startup = process.stderr.readline().decode().strip()
        assert startup.startswith("ACP listening on "), startup
        host, port = startup.removeprefix("ACP listening on ").rsplit(":", 1)
        request(process.stdin, process.stdout, 1, "initialize", {"protocolVersion": 1})
        session = request(process.stdin, process.stdout, 2, "session/new", {})["sessionId"]
        with socket.create_connection((host, int(port)), timeout=15) as connection:
            with connection.makefile("rwb", buffering=0) as stream:
                request(stream, stream, 1, "initialize", {"protocolVersion": 1})
                assert request(stream, stream, 2, "fx/turn/status", {"sessionId": session})["sessionId"] == session
                process.stdin.close()
                assert request(stream, stream, 3, "fx/turn/status", {"sessionId": session})["state"] == "idle"
        process.send_signal(signal.SIGINT)
        assert process.wait(timeout=15) == 0
        assert not process.stderr.read()
    finally:
        if process.poll() is None:
            process.kill()
            process.wait()
print("CLI model turn, stdio/listener shared state, independent detach, and shutdown passed")
