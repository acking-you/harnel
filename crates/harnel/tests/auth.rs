mod support;
use harnel::{Builder, Harness, LoginMethod, Provider, Value, json};
use std::{
    io::{BufRead, BufReader},
    process::{Child, Command, Stdio},
    time::Duration,
};
use support::acp::Client;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

struct OAuthFixture {
    process: Child,
    url: String,
}
const PYTHON: &str = if cfg!(windows) { "python" } else { "python3" };

impl OAuthFixture {
    fn start() -> Self {
        let started = std::time::Instant::now();
        let mut process = Command::new(PYTHON)
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/oauth_server.py"
            ))
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut url = String::new();
        BufReader::new(process.stdout.take().unwrap())
            .read_line(&mut url)
            .unwrap();
        eprintln!("OAuth fixture ready in {:?}", started.elapsed());
        Self {
            process,
            url: url.trim().into(),
        }
    }
    fn builder(&self, workspace: &std::path::Path) -> Builder {
        Harness::builder(workspace)
            .native_tools(false)
            .env("FX_E2E_CHATGPT_ISSUER_URL", &self.url)
            .env("FX_E2E_CHATGPT_TOKEN_URL", format!("{}/token", self.url))
            .env(
                "FX_E2E_OPENAI_CODEX_MODELS_URL",
                format!("{}/codex/models", self.url),
            )
            .env("FX_CODEX_ACCOUNT_BASE_URL", &self.url)
            .env("FX_E2E_GROK_ISSUER_URL", &self.url)
            .env("FX_E2E_GROK_TOKEN_URL", format!("{}/token", self.url))
            .env("FX_E2E_GROK_USERINFO_URL", format!("{}/userinfo", self.url))
            .env("FX_E2E_GROK_REVOKE_URL", format!("{}/revoke", self.url))
            .env(
                "FX_E2E_XAI_GROK_MODELS_URL",
                format!("{}/grok/models", self.url),
            )
            .env(
                "FX_E2E_XAI_GROK_MODALITIES_URL",
                format!("{}/grok/modalities", self.url),
            )
            .env("FX_E2E_XAI_GROK_BILLING_URL", format!("{}/usage", self.url))
    }
    async fn complete(&self, url: &str) {
        assert!(url.starts_with(&self.url));
        fixture_get(url).await;
    }
    async fn request_count(&self) -> u64 {
        let body = fixture_get(&format!("{}/requests", self.url)).await;
        serde_json::from_slice::<Value>(&body).unwrap()["count"]
            .as_u64()
            .unwrap()
    }
}
impl Drop for OAuthFixture {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}

// These fixtures use plain loopback HTTP and at most one browser callback
// redirect. Keep probes asynchronous and independent of Python startup and
// system proxy discovery so they do not consume the control-loop deadline.
async fn fixture_get(url: &str) -> Vec<u8> {
    let mut url = url.to_owned();
    for _ in 0..2 {
        let (authority, path) = url
            .strip_prefix("http://")
            .unwrap()
            .split_once('/')
            .unwrap();
        let address: std::net::SocketAddr = authority.parse().unwrap();
        assert!(address.ip().is_loopback());
        let mut socket = TcpStream::connect(address).await.unwrap();
        socket
            .write_all(
                format!("GET /{path} HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\n\r\n")
                    .as_bytes(),
            )
            .await
            .unwrap();
        let mut response = Vec::new();
        socket
            .take(64 * 1024)
            .read_to_end(&mut response)
            .await
            .unwrap();
        let response = String::from_utf8(response).unwrap();
        let (headers, body) = response.split_once("\r\n\r\n").unwrap();
        let status = headers
            .lines()
            .next()
            .unwrap()
            .split_whitespace()
            .nth(1)
            .unwrap();
        if status == "302" {
            url = headers
                .lines()
                .filter_map(|line| line.split_once(':'))
                .find(|(key, _)| key.eq_ignore_ascii_case("location"))
                .unwrap()
                .1
                .trim()
                .to_owned();
        } else {
            assert_eq!(status, "200", "fixture HTTP failed: {headers}");
            return body.as_bytes().to_vec();
        }
    }
    panic!("unexpected fixture redirect chain");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn device_codes_are_the_default_and_sdk_acp_share_authorization() {
    // Fixture provisioning is separate from the bounded native workflow.
    let fixture = OAuthFixture::start();
    tokio::time::timeout(Duration::from_secs(45), async {
        let workspace = tempfile::tempdir().unwrap();
        let harness = fixture.builder(workspace.path()).build().await.unwrap();
        let listener = harness
            .listen("127.0.0.1:0".parse().unwrap())
            .await
            .unwrap();
        let mut peer = Client::connect(listener.local_addr()).await;
        peer.initialize().await;
        for provider in [Provider::Codex, Provider::Grok] {
            let mut started = if provider == Provider::Codex {
                // Raw ACP omits method, exercising the embedded runtime default.
                let reply = peer
                    .request(
                        json!(2),
                        "fx/provider/login/start",
                        json!({"provider":provider}),
                    )
                    .await;
                assert!(reply.get("error").is_none(), "{reply}");
                reply["result"].clone()
            } else {
                harness.login(provider).await.unwrap()
            };
            while started["state"] == "preparing" {
                tokio::time::sleep(Duration::from_millis(25)).await;
                started = harness.login_status().await.unwrap();
            }
            assert_eq!(started["state"], "polling", "{started}");
            assert_eq!(started["method"], "device_code");
            assert_eq!(started["acceptsManualCode"], false);
            assert!(started["expiresIn"].as_u64().unwrap() > 0);
            let code = started["userCode"].as_str().unwrap();
            assert!(code.starts_with("TEST-"));
            assert!(!started.to_string().contains("private-device"));
            let observed = peer
                .request(json!(3), "fx/provider/login/status", json!({}))
                .await;
            assert_eq!(observed["result"]["userCode"], code);
            assert_eq!(harness.login_status().await.unwrap()["userCode"], code);
            // A separate HTTP client represents a browser on another device.
            fixture
                .complete(&format!(
                    "{}?user_code={code}",
                    started["verificationUri"].as_str().unwrap()
                ))
                .await;
            loop {
                let status = harness.login_status().await.unwrap();
                assert_ne!(status["state"], "failed", "{status}");
                if status["state"] == "succeeded" {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
            let observed = peer
                .request(json!(4), "fx/provider/login/status", json!({}))
                .await;
            assert_eq!(observed["result"]["state"], "succeeded");
            harness.switch_provider(provider).await.unwrap();
            assert_eq!(
                harness.provider_status().await.unwrap()["accountId"],
                "harnel-test-account"
            );
            assert_eq!(
                harness.refresh_credentials(provider).await.unwrap()["provider"],
                json!(provider)
            );
        }
        harness.switch_provider(Provider::Codex).await.unwrap();
        harness.logout(Provider::Codex).await.unwrap();
        harness.logout(Provider::Grok).await.unwrap();
        listener.shutdown().await.unwrap();
        harness.shutdown().await.unwrap();
    })
    .await
    .expect("device authorization timed out");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn device_request_does_not_block_acp_status_or_cancellation() {
    // Fixture provisioning is separate from the bounded native workflow.
    let fixture = OAuthFixture::start();
    tokio::time::timeout(Duration::from_secs(30), async {
        let workspace = tempfile::tempdir().unwrap();
        let harness = fixture
            .builder(workspace.path())
            .env("FX_E2E_GROK_ISSUER_URL", format!("{}/delayed", fixture.url))
            .build()
            .await
            .unwrap();
        let listener = harness
            .listen("127.0.0.1:0".parse().unwrap())
            .await
            .unwrap();
        let mut peer = Client::connect(listener.local_addr()).await;
        peer.initialize().await;
        let started = peer
            .request(
                json!(2),
                "fx/provider/login/start",
                json!({"provider":"grok"}),
            )
            .await;
        assert_eq!(started["result"]["state"], "preparing");
        while fixture.request_count().await == 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        // The provider has accepted the request but holds its response for 3s.
        tokio::time::timeout(Duration::from_secs(1), async {
            assert_eq!(harness.login_status().await.unwrap()["state"], "preparing");
            let cancelled = peer
                .request(json!(3), "fx/provider/login/cancel", json!({}))
                .await;
            assert_eq!(cancelled["result"]["cancelled"], true);
        })
        .await
        .expect("provider HTTP blocked the ACP read loop");
        loop {
            let status = harness.login_status().await.unwrap();
            if status["state"] == "cancelled" {
                break;
            }
            assert_eq!(status["state"], "preparing", "{status}");
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        assert!(!harness.state_dir().join(".fx/grok-auth.json").exists());
        // A completed cancellation releases the owner for another login.
        let retried = harness.login(Provider::Codex).await.unwrap();
        assert_eq!(retried["state"], "polling", "{retried}");
        harness.cancel_login().await.unwrap();
        assert_eq!(harness.login_status().await.unwrap()["state"], "cancelled");
        listener.shutdown().await.unwrap();
        harness.shutdown().await.unwrap();
    })
    .await
    .expect("device cancellation timed out");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn codex_and_grok_oauth_complete_and_remain_observable() {
    // Fixture provisioning is separate from the bounded native workflow.
    let fixture = OAuthFixture::start();
    tokio::time::timeout(Duration::from_secs(45), async {
        let workspace = tempfile::tempdir().unwrap();
        let harness = fixture.builder(workspace.path()).build().await.unwrap();
        for provider in [Provider::Codex, Provider::Grok] {
            let started = harness
                .login_with_method(provider, LoginMethod::Browser)
                .await
                .unwrap();
            assert_eq!(started["state"], "polling");
            fixture
                .complete(started["authorizationUrl"].as_str().unwrap())
                .await;
            loop {
                let status = harness.login_status().await.unwrap();
                assert_ne!(status["state"], "failed", "{status}");
                if status["state"] == "succeeded" {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
            assert_eq!(
                harness.clone().login_status().await.unwrap()["state"],
                "succeeded"
            );
        }
        let selected = harness.switch_provider(Provider::Codex).await.unwrap();
        assert_eq!(selected["provider"], "codex");
        assert_eq!(
            harness.provider_status().await.unwrap()["accountId"],
            "harnel-test-account"
        );
        assert_eq!(
            harness.refresh_credentials(Provider::Codex).await.unwrap()["provider"],
            "codex"
        );
        assert_eq!(
            harness.switch_provider(Provider::Grok).await.unwrap()["provider"],
            "grok"
        );
        assert_eq!(
            harness.refresh_credentials(Provider::Grok).await.unwrap()["provider"],
            "grok"
        );
        harness.logout(Provider::Codex).await.unwrap();
        assert_eq!(
            harness.provider_status().await.unwrap()["authenticated"],
            true
        );
        harness.logout(Provider::Grok).await.unwrap();
        assert_eq!(
            harness.provider_status().await.unwrap()["authenticated"],
            false
        );
        assert!(!harness.state_dir().join(".fx/chatgpt-auth.json").exists());
        assert!(!harness.state_dir().join(".fx/grok-auth.json").exists());
        harness.shutdown().await.unwrap();
    })
    .await
    .expect("OAuth flow timed out");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn byok_configuration_logout_and_saved_session_removal() {
    let model = support::Model::start(|_, _| support::text("saved answer")).await;
    let root = tempfile::tempdir().unwrap();
    let harness = Harness::builder(root.path())
        .native_tools(false)
        .build()
        .await
        .unwrap();
    harness
        .configure_provider(&model.url, "fixture-api-key")
        .await
        .unwrap();
    assert_eq!(
        harness.provider_status().await.unwrap()["authenticated"],
        true
    );
    let session = harness.session().await.unwrap();
    session.prompt("create a saved turn").await.unwrap();
    session.remove().await.unwrap();
    assert!(harness.load_session(session.id()).await.is_err());
    harness.logout(Provider::Gateway).await.unwrap();
    assert_eq!(
        harness.provider_status().await.unwrap()["authenticated"],
        false
    );
    assert!(!harness.state_dir().join(".fx/gateway-auth.json").exists());
    harness.shutdown().await.unwrap();
}
