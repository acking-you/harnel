#!/usr/bin/env python3
"""Run the documented applications against a deterministic Responses server."""
import argparse
import asyncio
import json
import os
from pathlib import Path
import signal
import subprocess
import tempfile
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


def completed():
    return {"type": "response.completed", "response": {"status": "completed",
            "usage": {"input_tokens": 8, "output_tokens": 5}}}


def text_events(text):
    middle = len(text) // 2
    return [{"type": "response.output_text.delta", "item_id": "answer", "output_index": 0,
             "content_index": 0, "delta": part} for part in (text[:middle], text[middle:])] + [completed()]


def tool_events(name, arguments):
    return [
        {"type": "response.output_item.added", "output_index": 0,
         "item": {"type": "function_call", "id": "tool_1", "call_id": "call_1", "name": name}},
        {"type": "response.function_call_arguments.done", "item_id": "tool_1", "call_id": "call_1",
         "output_index": 0, "arguments": json.dumps(arguments)}, completed(),
    ]


class Fixture(ThreadingHTTPServer):
    daemon_threads = True

    def __init__(self):
        super().__init__(("127.0.0.1", 0), Handler)
        self.mode = "text"
        self.errors = []
        self.requests = []
        self.started = threading.Event()
        self.release = threading.Event()

    def reply(self, body):
        self.requests.append(body)
        if self.mode == "resume":
            assert "Remember that my project is named Atlas" in json.dumps(body["input"])
        if self.mode in ("inventory", "notebook"):
            name, arguments, expected = (
                ("lookup_inventory", {"sku": "keyboard"}, '"quantity":7')
                if self.mode == "inventory" else
                ("read_note", {"id": "launch.md"}, "240 active teams")
            )
            outputs = [item for item in body["input"] if item.get("type") == "function_call_output"]
            if not outputs:
                assert any(tool.get("name") == name for tool in body["tools"])
                return tool_events(name, arguments)
            assert expected in outputs[-1]["output"], outputs[-1]
            return text_events("Seven keyboards are available." if self.mode == "inventory" else
                               "Atlas launches on October 15, owned by Maya; 240 active teams. [launch.md]")
        return text_events("EXAMPLE_SMOKE_OK")


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_GET(self):
        body = json.dumps({"data": [{"id": "gpt-5", "object": "model", "owned_by": "openai"}]}).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_POST(self):
        try:
            body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
            assert self.path == "/v1/responses", self.path
            events = self.server.reply(body)
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.send_header("Connection", "close")
            self.end_headers()
            for event in events:
                self.wfile.write(f"data: {json.dumps(event)}\n\n".encode())
                self.wfile.flush()
                if self.server.mode == "slow":
                    self.server.started.set()
                    self.server.release.wait(30)
            self.wfile.write(b"data: [DONE]\n\n")
        except (BrokenPipeError, ConnectionResetError, ConnectionAbortedError):
            pass
        except Exception as error:
            self.server.errors.append(repr(error))
            self.close_connection = True


def run(binary, args, root, environment):
    result = subprocess.run([str(binary), *args], cwd=root, env=environment,
                            text=True, capture_output=True, timeout=45)
    assert result.returncode == 0, (result.stdout, result.stderr)
    return result


def process_options():
    return {"creationflags": subprocess.CREATE_NEW_PROCESS_GROUP} if os.name == "nt" else {}


def interrupt(process):
    process.send_signal(signal.CTRL_BREAK_EVENT if os.name == "nt" else signal.SIGINT)


async def request(write, read, number, method, params):
    write.write((json.dumps({"jsonrpc": "2.0", "id": number, "method": method, "params": params}) + "\n").encode())
    await write.drain()
    while True:
        line = await asyncio.wait_for(read.readline(), 15)
        assert line, f"EOF waiting for {method}"
        value = json.loads(line)
        if value.get("id") == number:
            assert "error" not in value, value
            return value["result"]


async def acp(binary, root, environment):
    process = await asyncio.create_subprocess_exec(
        str(binary), cwd=root, env={**environment, "HARNEL_LISTEN": "127.0.0.1:0"},
        stdin=asyncio.subprocess.PIPE, stdout=asyncio.subprocess.PIPE,
        stderr=asyncio.subprocess.PIPE, limit=1024 * 1024, **process_options())
    try:
        address = (await asyncio.wait_for(process.stderr.readline(), 15)).decode().strip()
        session = (await asyncio.wait_for(process.stderr.readline(), 15)).decode().strip()
        assert address.startswith("ACP listening on "), address
        assert session.startswith("SDK session: "), session
        session = session.removeprefix("SDK session: ")
        host, port = address.removeprefix("ACP listening on ").rsplit(":", 1)
        await request(process.stdin, process.stdout, 1, "initialize", {"protocolVersion": 1})
        read, write = await asyncio.wait_for(asyncio.open_connection(host, int(port)), 15)
        try:
            await request(write, read, 1, "initialize", {"protocolVersion": 1})
            assert (await request(write, read, 2, "fx/turn/status", {"sessionId": session}))["sessionId"] == session
            assert (await request(process.stdin, process.stdout, 2, "fx/turn/status", {"sessionId": session}))["state"] == "idle"
            process.stdin.close()
            await process.stdin.wait_closed()
            assert (await request(write, read, 4, "fx/turn/status", {"sessionId": session}))["state"] == "idle"
        finally:
            write.close()
            await write.wait_closed()
        interrupt(process)
        assert await asyncio.wait_for(process.wait(), 15) == 0
        assert not await process.stderr.read()
    finally:
        if process.returncode is None:
            process.kill()
            await process.wait()


def examples(directory, root, environment, server):
    suffix = ".exe" if os.name == "nt" else ""
    binary = lambda name: directory / (name + suffix)
    for name in ("embed", "stream", "provider"):
        result = run(binary(name), [], root, environment)
        assert result.stdout.strip() == "EXAMPLE_SMOKE_OK", result.stdout
        if name == "provider":
            assert 'Active provider: "gateway"' in result.stderr, result.stderr
    first = run(binary("sessions"), [], root, environment)
    session = next(line.removeprefix("Session: ") for line in first.stderr.splitlines() if line.startswith("Session: "))
    server.mode = "resume"
    second = run(binary("sessions"), ["What is my project called?"], root,
                 {**environment, "HARNEL_SESSION_ID": session})
    assert f"Session: {session}" in second.stderr
    server.mode = "inventory"
    assert run(binary("custom_tool"), [], root, environment).stdout.strip() == "Seven keyboards are available."
    server.mode = "slow"
    process = subprocess.Popen([str(binary("stream"))], cwd=root, env=environment,
                               text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, **process_options())
    try:
        assert server.started.wait(15), "The stream did not start"
        interrupt(process)
        stdout, stderr = process.communicate(timeout=30)
        assert process.returncode == 0, (stdout, stderr)
        assert "Stop reason: cancelled" in stderr, stderr
    finally:
        server.release.set()
        if process.poll() is None:
            process.kill()
            stdout, stderr = process.communicate(timeout=15)
            print("Stream cancellation diagnostics:", stdout, stderr, flush=True)
    asyncio.run(acp(binary("acp"), root, environment))
    print("Examples passed: answer, streaming, cancellation, Rust tool, provider, saved-session resume, shared SDK/ACP.")


def notebook(binary, root, environment, server):
    notes = root / "notes"
    notes.mkdir()
    (notes / "launch.md").write_text("# Atlas\nAtlas launches on October 15. Maya owns it. The beta has 240 active teams.\n")
    server.mode = "notebook"
    first = run(binary, [str(notes), "Read launch.md and summarize the launch."], root, environment)
    assert "October 15" in first.stdout and "240 active teams" in first.stdout, first.stdout
    session = next(line.removeprefix("Session: ") for line in first.stderr.splitlines() if line.startswith("Session: "))
    assert len(server.requests) == 2, server.requests
    second = run(binary, [str(notes), "Who owns the launch?", session], root, environment)
    assert f"Session: {session}" in second.stderr, second.stderr
    assert "Read launch.md and summarize the launch." in json.dumps(server.requests[-1]["input"])
    assert "Maya" in second.stdout, second.stdout
    assert (Path(environment["HARNEL_STATE_DIR"]) / ".fx/sessions" / session / "session.json").is_file()
    print(first.stdout.strip())
    print(first.stderr.strip())
    print("Notebook passed: Markdown input, native Rust tool round trip, streamed answer, process restart, saved conversation.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument("--examples", type=Path)
    group.add_argument("--notebook", type=Path)
    options = parser.parse_args()
    server = Fixture()
    thread = threading.Thread(target=server.serve_forever)
    thread.start()
    try:
        with tempfile.TemporaryDirectory(prefix="harnel-example-") as temporary:
            root = Path(temporary)
            environment = {key: value for key, value in os.environ.items() if not key.startswith("HARNEL_")}
            environment.update(HARNEL_MODEL="openai/gpt-5", HARNEL_API_KEY="synthetic-fixture-key",
                               HARNEL_BASE_URL=f"http://127.0.0.1:{server.server_port}/v1",
                               HARNEL_STATE_DIR=str(root / "profile"))
            if options.examples:
                examples(options.examples.resolve(), root, environment, server)
            else:
                notebook(options.notebook.resolve(), root, environment, server)
            assert not server.errors, server.errors
    finally:
        server.release.set()
        server.shutdown()
        server.server_close()
        thread.join()


if __name__ == "__main__":
    main()
