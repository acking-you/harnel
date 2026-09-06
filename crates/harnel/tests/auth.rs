mod support;
use harnel::{Harness, Provider};
use std::{
    io::{BufRead, BufReader},
    process::{Child, Command, Stdio},
    time::Duration,
};

struct OAuthFixture {
    process: Child,
    url: String,
}
impl OAuthFixture {
    fn start() -> Self {
        let mut process = Command::new("python3")
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
    fn complete(&self, url: &str) {
        assert!(url.starts_with(&self.url));
        let status = Command::new("python3")
            .args([
                "-c",
                "import sys,urllib.request; urllib.request.urlopen(sys.argv[1],timeout=10).read()",
                url,
            ])
            .status()
            .unwrap();
        assert!(status.success());
    }
}
impl Drop for OAuthFixture {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn codex_and_grok_oauth_complete_and_remain_observable() {
    tokio::time::timeout(Duration::from_secs(45), async {
        let fixture = OAuthFixture::start();
        let workspace = tempfile::tempdir().unwrap();
        let harness = Harness::builder(workspace.path())
            .native_tools(false)
            .env("FX_E2E_CHATGPT_ISSUER_URL", &fixture.url)
            .env("FX_E2E_CHATGPT_TOKEN_URL", format!("{}/token", fixture.url))
            .env(
                "FX_E2E_OPENAI_CODEX_MODELS_URL",
                format!("{}/codex/models", fixture.url),
            )
            .env("FX_CODEX_ACCOUNT_BASE_URL", &fixture.url)
            .env("FX_E2E_GROK_ISSUER_URL", &fixture.url)
            .env("FX_E2E_GROK_TOKEN_URL", format!("{}/token", fixture.url))
            .env(
                "FX_E2E_GROK_USERINFO_URL",
                format!("{}/userinfo", fixture.url),
            )
            .env("FX_E2E_GROK_REVOKE_URL", format!("{}/revoke", fixture.url))
            .env(
                "FX_E2E_XAI_GROK_MODELS_URL",
                format!("{}/grok/models", fixture.url),
            )
            .env(
                "FX_E2E_XAI_GROK_MODALITIES_URL",
                format!("{}/grok/modalities", fixture.url),
            )
            .env(
                "FX_E2E_XAI_GROK_BILLING_URL",
                format!("{}/usage", fixture.url),
            )
            .build()
            .await
            .unwrap();
        for provider in [Provider::Codex, Provider::Grok] {
            let started = harness.login(provider).await.unwrap();
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
