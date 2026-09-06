"""Local OAuth provider fixture. Every token and identity is synthetic."""
import base64
import json
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlencode, urlsplit


def token():
    claims = {"exp": 4102444800, "sub": "harnel-test-account",
              "https://api.openai.com/auth": {"chatgpt_account_id": "harnel-test-account"}}
    payload = base64.urlsafe_b64encode(json.dumps(claims).encode()).rstrip(b"=").decode()
    return f"header.{payload}.fixture"


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def respond(self, body):
        body = json.dumps(body).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        path = urlsplit(self.path)
        query = parse_qs(path.query)
        if "redirect_uri" in query:
            callback = query["redirect_uri"][0].replace("localhost", "127.0.0.1")
            self.send_response(302)
            self.send_header("Location", callback + "?" + urlencode({"state": query["state"][0], "code": "fixture-code"}))
            self.end_headers()
        elif path.path == "/userinfo":
            self.respond({"sub": "harnel-test-account"})
        elif path.path == "/codex/models":
            self.respond({"models": [{"slug": model, "visibility": "list", "supported_in_api": True,
                                      "supported_reasoning_levels": [{"effort": "high"}], "context_window": 272000,
                                      "additional_speed_tiers": [], "input_modalities": ["text"]}
                                     for model in ["gpt-5.6-sol", "gpt-5.4-mini"]]})
        elif path.path == "/usage":
            self.respond({})
        elif path.path == "/grok/models":
            self.respond({"data": [{"id": "grok-4.20", "model": "grok-4.20", "api_backend": "responses",
                                    "context_window": 1000000, "supports_reasoning_effort": False, "reasoning_efforts": []}]})
        elif path.path == "/grok/modalities":
            self.respond({"models": [{"id": "grok-4.20", "input_modalities": ["text"], "output_modalities": ["text"]}]})
        else:
            self.send_error(404)

    def do_POST(self):
        self.rfile.read(int(self.headers.get("Content-Length", "0")))
        if self.path == "/token":
            self.respond({"access_token": token(), "refresh_token": "fixture-refresh", "expires_in": 3600})
        elif self.path == "/revoke":
            self.respond({})
        else:
            self.send_error(404)


server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
print(f"http://127.0.0.1:{server.server_port}", flush=True)
server.serve_forever()
