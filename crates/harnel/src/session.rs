use crate::{Error, Harness, Provider, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// A handle to a saved native session. A harness has one loaded session at a
/// time; loading or creating another session changes that shared selection.
#[derive(Clone)]
pub struct Session {
    harness: Harness,
    id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnResult {
    pub stop_reason: String,
    #[serde(flatten)]
    pub metadata: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone)]
pub struct Answer {
    pub text: String,
    pub turn: TurnResult,
}

/// Device codes work without a browser or callback listener on the host.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoginMethod {
    DeviceCode,
    Browser,
}

impl Harness {
    pub async fn session(&self) -> Result<Session> {
        let result = self
            .request(
                "session/new",
                json!({"cwd":self.workspace(),"mcpServers":[]}),
            )
            .await?;
        let id = result["sessionId"]
            .as_str()
            .ok_or_else(|| Error::Invalid("native session response lacks sessionId".into()))?
            .to_owned();
        Ok(Session {
            harness: self.clone(),
            id,
        })
    }
    pub async fn load_session(&self, id: impl Into<String>) -> Result<Session> {
        let id = id.into();
        self.request(
            "session/load",
            json!({"sessionId":id,"cwd":self.workspace(),"mcpServers":[]}),
        )
        .await?;
        Ok(Session {
            harness: self.clone(),
            id,
        })
    }
    /// Attach an SDK handle to the session already loaded by an ACP peer.
    pub fn session_handle(&self, id: impl Into<String>) -> Session {
        Session {
            harness: self.clone(),
            id: id.into(),
        }
    }
    pub async fn list_sessions(&self) -> Result<Value> {
        self.request("session/list", json!({})).await
    }
    pub async fn switch_provider(&self, provider: Provider) -> Result<Value> {
        self.request("fx/provider/switch", json!({"provider":provider}))
            .await
    }
    pub async fn provider_status(&self) -> Result<Value> {
        self.request("fx/provider/status", json!({})).await
    }
    pub async fn logout(&self, provider: Provider) -> Result<Value> {
        self.request("fx/provider/logout", json!({"provider":provider}))
            .await
    }
    pub async fn refresh_credentials(&self, provider: Provider) -> Result<Value> {
        self.request("fx/provider/refresh", json!({"provider":provider}))
            .await
    }
    pub async fn configure_provider(&self, base_url: &str, api_key: &str) -> Result<Value> {
        self.request(
            "fx/provider/configure",
            json!({"baseUrl":base_url,"apiKey":api_key}),
        )
        .await
    }
    pub async fn login(&self, provider: Provider) -> Result<Value> {
        self.login_with_method(provider, LoginMethod::DeviceCode)
            .await
    }
    /// Wait for the authorization URL/code to become available. Authorization
    /// itself remains observable through `login_status` from every attachment.
    pub async fn login_with_method(
        &self,
        provider: Provider,
        method: LoginMethod,
    ) -> Result<Value> {
        let mut status = self
            .request(
                "fx/provider/login/start",
                json!({"provider":provider,"method":method}),
            )
            .await?;
        tokio::time::timeout(std::time::Duration::from_secs(60), async {
            while status["state"] == "preparing" {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                status = self.login_status().await?;
            }
            Ok(status)
        })
        .await
        .map_err(|_| Error::Timeout)?
    }
    pub async fn login_status(&self) -> Result<Value> {
        self.request("fx/provider/login/status", json!({})).await
    }
    pub async fn submit_login_code(&self, code: &str) -> Result<Value> {
        self.request("fx/provider/login/submitCode", json!({"code":code}))
            .await
    }
    pub async fn cancel_login(&self) -> Result<Value> {
        let result = self.request("fx/provider/login/cancel", json!({})).await?;
        if result["cancelled"] == true {
            tokio::time::timeout(std::time::Duration::from_secs(60), async {
                loop {
                    let status = self.login_status().await?;
                    if status["state"] != "preparing" && status["state"] != "polling" {
                        return Ok::<(), Error>(());
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                }
            })
            .await
            .map_err(|_| Error::Timeout)??;
        }
        Ok(result)
    }
    pub async fn provider_usage(&self) -> Result<Value> {
        self.request("fx/provider/usage", json!({})).await
    }
    pub async fn bash_first(&self, enabled: bool) -> Result<Value> {
        self.request("fx/toolMode/set", json!({"bashFirst":enabled}))
            .await
    }

    /// Import credentials from supported Codex/Grok CLI stores in this profile.
    /// Use explicit environment entries to point at an existing CLI profile.
    pub async fn import_cli_credentials(&self) -> Result<Value> {
        self.request("fx/provider/setup/start", json!({})).await
    }
    pub async fn credential_import_status(&self) -> Result<Value> {
        self.request("fx/provider/setup/status", json!({})).await
    }
}

impl Session {
    /// Convenience interface that collects assistant text. Use `prompt` and
    /// `Harness::subscribe` when the application needs streaming events.
    pub async fn ask(&self, text: impl Into<String>) -> Result<Answer> {
        let mut events = self.harness.subscribe();
        let turn = self.prompt(text);
        tokio::pin!(turn);
        let mut text = String::new();
        loop {
            tokio::select! {
                result = &mut turn => {
                    let turn = result?;
                    while let Some(event) = events.try_recv()? { self.collect_text(&mut text, &event)?; }
                    return Ok(Answer { text, turn });
                },
                event = events.recv() => self.collect_text(&mut text, &event?)?,
            }
        }
    }

    fn collect_text(&self, text: &mut String, event: &crate::Event) -> Result<()> {
        if event.params["sessionId"] == self.id
            && event.params["update"]["sessionUpdate"] == "agent_message_chunk"
        {
            if let Some(chunk) = event.params["update"]["content"]["text"].as_str() {
                if text.len() + chunk.len() > 8 * 1024 * 1024 {
                    return Err(Error::Invalid(
                        "collected answer exceeds 8 MiB; use streaming".into(),
                    ));
                }
                text.push_str(chunk);
            }
        }
        Ok(())
    }
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn harness(&self) -> &Harness {
        &self.harness
    }
    pub async fn prompt(&self, text: impl Into<String>) -> Result<TurnResult> {
        self.prompt_content(vec![json!({"type":"text","text":text.into()})])
            .await
    }
    /// Accepts ACP content blocks, including text, images, and resources.
    pub async fn prompt_content(&self, content: Vec<Value>) -> Result<TurnResult> {
        Ok(serde_json::from_value(
            self.harness
                .request(
                    "session/prompt",
                    json!({"sessionId":self.id,"prompt":content}),
                )
                .await?,
        )?)
    }
    pub fn cancel(&self) -> Result<()> {
        self.harness
            .notify("session/cancel", json!({"sessionId":self.id}))
    }
    /// Send input to the exact active turn reported by `status`.
    pub async fn steer(&self, expected_turn_id: &str, text: &str) -> Result<Value> {
        self.harness.request("fx/turn/steer", json!({"sessionId":self.id,"expectedTurnId":expected_turn_id,"input":[{"type":"text","text":text}]})).await
    }
    pub async fn status(&self) -> Result<Value> {
        self.harness
            .request("fx/turn/status", json!({"sessionId":self.id}))
            .await
    }
    pub async fn set_option(&self, key: &str, value: impl Into<Value>) -> Result<Value> {
        self.harness
            .request(
                "session/set_config_option",
                json!({"sessionId":self.id,"configId":key,"value":value.into()}),
            )
            .await
    }
    pub async fn set_mode(&self, mode: &str) -> Result<Value> {
        self.harness
            .request(
                "session/set_mode",
                json!({"sessionId":self.id,"modeId":mode}),
            )
            .await
    }
    pub async fn compact(&self) -> Result<TurnResult> {
        self.prompt("/compact").await
    }
    pub async fn close(&self) -> Result<Value> {
        self.harness
            .request("session/close", json!({"sessionId":self.id}))
            .await
    }
    pub async fn remove(&self) -> Result<Value> {
        self.harness
            .request("session/remove", json!({"sessionId":self.id}))
            .await
    }
    pub async fn write_stdin(&self, process_id: u64, chars: &str) -> Result<Value> {
        self.harness
            .request(
                "fx/unifiedExec/writeStdin",
                json!({"sessionId":self.id,"processId":process_id,"chars":chars}),
            )
            .await
    }
    pub async fn kill_process(&self, process_id: u64) -> Result<Value> {
        self.harness
            .request(
                "fx/unifiedExec/kill",
                json!({"sessionId":self.id,"processId":process_id}),
            )
            .await
    }
}
