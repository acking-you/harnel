mod support;
use harnel::{Builder, Harness, LoginMethod, Provider, Value, json};
use std::{
    io::{BufRead, BufReader},
    process::{Child, Command, Stdio},
    time::Duration,
};
use support::acp::Client;

struct OAuthFixture {
    process: Child,
    url: String,
}
const PYTHON: &str = if cfg!(windows) { "python" } else { "python3" };

impl OAuthFixture {
    fn start() -> Self {
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
    fn complete(&self, url: &str) {
        assert!(url.starts_with(&self.url));
        let status = Command::new(PYTHON)
            .args([
                "-c",
                "import sys,urllib.request; urllib.request.urlopen(sys.argv[1],timeout=10).read()",
                url,
            ])
            .status()
            .unwrap();
        assert!(status.success());
    }
    fn request_count(&self) -> u64 {
        let output = Command::new(PYTHON)
            .args([
                "-c",
                "import sys,urllib.request; print(urllib.request.urlopen(sys.argv[1],timeout=5).read().decode())",
                &format!("{}/requests", self.url),
            ])
            .output()
            .unwrap();
        assert!(output.status.success());
        serde_json::from_slice::<Value>(&output.stdout).unwrap()["count"]
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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn device_codes_are_the_default_and_sdk_acp_share_authorization() {
    tokio::time::timeout(Duration::from_secs(45), async {
        let fixture = OAuthFixture::start();
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
            fixture.complete(&format!(
                "{}?user_code={code}",
                started["verificationUri"].as_str().unwrap()
            ));
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
    tokio::time::timeout(Duration::from_secs(30), async {
        let fixture = OAuthFixture::start();
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
        while fixture.request_count() == 0 {
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
    tokio::time::timeout(Duration::from_secs(45), async {
        let fixture = OAuthFixture::start();
        let workspace = tempfile::tempdir().unwrap();
        let harness = fixture.builder(workspace.path()).build().await.unwrap();
        for provider in [Provider::Codex, Provider::Grok] {
            let started = harness
                .login_with_method(provider, LoginMethod::Browser)
                .await
                .unwrap();
            assert_eq!(started["state"], "polling");
            fixture.complete(started["authorizationUrl"].as_str().unwrap());
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
