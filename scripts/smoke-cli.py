#!/usr/bin/env python3
"""Exercise the real CLI against local model and ACP transports."""
import asyncio
import json
import os
import pathlib
import signal
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


async def request(write, read, number, method, params):
    write.write((json.dumps({"jsonrpc": "2.0", "id": number, "method": method, "params": params}) + "\n").encode())
    await write.drain()
    while True:
        line = await asyncio.wait_for(read.readline(), timeout=15)
        if not line:
            raise RuntimeError(f"EOF waiting for {method}")
        value = json.loads(line)
        if value.get("id") == number:
            assert "error" not in value, value
            return value["result"]


async def acp_smoke(root):
    options = {"creationflags": subprocess.CREATE_NEW_PROCESS_GROUP} if os.name == "nt" else {}
    process = await asyncio.create_subprocess_exec(
        binary, "acp", "--stdio", "--listen", "127.0.0.1:0", "--workspace", root,
        stdin=asyncio.subprocess.PIPE, stdout=asyncio.subprocess.PIPE,
        stderr=asyncio.subprocess.PIPE, limit=1024 * 1024, **options)
    try:
        startup = (await asyncio.wait_for(process.stderr.readline(), timeout=15)).decode().strip()
        assert startup.startswith("ACP listening on "), startup
        host, port = startup.removeprefix("ACP listening on ").rsplit(":", 1)
        await request(process.stdin, process.stdout, 1, "initialize", {"protocolVersion": 1})
        session = (await request(process.stdin, process.stdout, 2, "session/new", {}))["sessionId"]
        read, write = await asyncio.wait_for(asyncio.open_connection(host, int(port)), timeout=15)
        try:
            await request(write, read, 1, "initialize", {"protocolVersion": 1})
            assert (await request(write, read, 2, "fx/turn/status", {"sessionId": session}))["sessionId"] == session
            process.stdin.close()
            await process.stdin.wait_closed()
            assert (await request(write, read, 3, "fx/turn/status", {"sessionId": session}))["state"] == "idle"
        finally:
            write.close()
            await write.wait_closed()
        process.send_signal(signal.CTRL_BREAK_EVENT if os.name == "nt" else signal.SIGINT)
        assert await asyncio.wait_for(process.wait(), timeout=15) == 0
        assert not await process.stderr.read()
    finally:
        if process.returncode is None:
            process.kill()
            await process.wait()


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

    asyncio.run(acp_smoke(root))
print("CLI model turn, stdio/listener shared state, independent detach, and shutdown passed")
