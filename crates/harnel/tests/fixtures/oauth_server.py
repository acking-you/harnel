"""Local OAuth provider fixture. Every token and identity is synthetic."""
import base64
import json
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlencode, urlsplit

devices = {}
lock = threading.Lock()
requests = 0


def token():
    claims = {"exp": 4102444800, "sub": "harnel-test-account",
              "https://api.openai.com/auth": {"chatgpt_account_id": "harnel-test-account"}}
    payload = base64.urlsafe_b64encode(json.dumps(claims).encode()).rstrip(b"=").decode()
    return f"header.{payload}.fixture"


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def respond(self, body, status=200):
        body = json.dumps(body).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        try:
            self.wfile.write(body)
        except (BrokenPipeError, ConnectionResetError):
            pass  # Cancellation may close a delayed response.

    def do_GET(self):
        path = urlsplit(self.path)
        query = parse_qs(path.query)
        if path.path == "/requests":
            with lock:
                self.respond({"count": requests})
        elif path.path in ("/codex/device", "/grok/device"):
            with lock:
                code = query.get("user_code", [""])[0]
                assert code in devices, code
                devices[code]["approved"] = True
            self.respond({"approved": True})
        elif "redirect_uri" in query:
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
        global requests
        raw = self.rfile.read(int(self.headers.get("Content-Length", "0")))
        if self.headers.get("Content-Type", "").startswith("application/json"):
            body = json.loads(raw)
        else:
            body = {key: values[0] for key, values in parse_qs(raw.decode()).items()}
        if self.path.endswith(("/api/accounts/deviceauth/usercode", "/oauth2/device/code")):
            assert body.get("client_id")
            grok = self.path.endswith("/oauth2/device/code")
            if grok:
                assert body["referrer"] == "grok-build"
                assert "offline_access" in body["scope"]
            with lock:
                requests += 1
                code = f"TEST-{requests:04d}"
                private = f"private-device-{requests}"
                devices[code] = {"private": private, "approved": False}
            if self.path.startswith("/delayed/"):
                time.sleep(3)
            if grok:
                self.respond({"device_code": private, "user_code": code,
                              "verification_uri": origin + "/grok/device",
                              "verification_uri_complete": origin + "/grok/device?user_code=" + code,
                              "expires_in": 900, "interval": 1})
            else:
                self.respond({"device_auth_id": private, "user_code": code, "interval": "1"})
            return
        if self.path == "/api/accounts/deviceauth/token":
            with lock:
                device = devices[body["user_code"]]
                assert body["device_auth_id"] == device["private"]
                approved = device["approved"]
            if not approved:
                self.respond({"error": "authorization_pending"}, 403)
            else:
                self.respond({"authorization_code": "device-authorization-code",
                              "code_verifier": "device-pkce-verifier", "code_challenge": "fixture-challenge"})
            return
        if self.path == "/token":
            if body.get("grant_type") == "urn:ietf:params:oauth:grant-type:device_code":
                with lock:
                    device = next(d for d in devices.values() if d["private"] == body["device_code"])
                    approved = device["approved"]
                if not approved:
                    self.respond({"error": "authorization_pending"}, 400)
                    return
            elif body.get("code") == "device-authorization-code":
                assert body["grant_type"] == "authorization_code"
                assert body["code_verifier"] == "device-pkce-verifier"
                assert body["redirect_uri"] == origin + "/deviceauth/callback"
            self.respond({"access_token": token(), "refresh_token": "fixture-refresh", "expires_in": 3600})
        elif self.path == "/revoke":
            self.respond({})
        else:
            self.send_error(404)


server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
origin = f"http://127.0.0.1:{server.server_port}"
print(origin, flush=True)
server.serve_forever()
