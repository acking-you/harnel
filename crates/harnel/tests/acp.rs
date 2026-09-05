mod support;
use harnel::{Harness, Value, json};
use std::time::Duration;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{
        TcpStream,
        tcp::{OwnedReadHalf, OwnedWriteHalf},
    },
};

struct Client {
    read: BufReader<OwnedReadHalf>,
    write: OwnedWriteHalf,
}
impl Client {
    async fn connect(address: std::net::SocketAddr) -> Self {
        let socket = TcpStream::connect(address).await.unwrap();
        let (read, write) = socket.into_split();
        Self {
            read: BufReader::new(read),
            write,
        }
    }
    async fn send(&mut self, message: Value) {
        self.write
            .write_all(format!("{message}\n").as_bytes())
            .await
            .unwrap();
    }
    async fn reply(&mut self, id: Value) -> Value {
        loop {
            let mut line = String::new();
            assert!(self.read.read_line(&mut line).await.unwrap() > 0);
            let value: Value = serde_json::from_str(&line).unwrap();
            if value.get("id") == Some(&id) {
                return value;
            }
        }
    }
    async fn request(&mut self, id: Value, method: &str, params: Value) -> Value {
        self.send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
            .await;
        self.reply(id).await
    }
    async fn initialize(&mut self) {
        assert_eq!(
            self.request(
                json!(1),
                "initialize",
                json!({"protocolVersion":1,"clientCapabilities":{}})
            )
            .await["result"]["protocolVersion"],
            1
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sdk_and_two_acp_peers_share_session_and_disconnect_independently() {
    tokio::time::timeout(Duration::from_secs(30), async {
        let model = support::Model::start(|_, _| support::text("shared session reply")).await;
        let workspace = tempfile::tempdir().unwrap();
        let harness = Harness::builder(workspace.path()).native_tools(false).model("openai/gpt-5").base_url(&model.url).api_key("test-key").build().await.unwrap();
        let listener = harness.listen("127.0.0.1:0".parse().unwrap()).await.unwrap();
        let mut first = Client::connect(listener.local_addr()).await;
        let mut second = Client::connect(listener.local_addr()).await;
        first.initialize().await;
        second.initialize().await;
        let created = first.request(json!(2), "session/new", json!({"cwd":workspace.path(),"mcpServers":[]})).await;
        let id = created["result"]["sessionId"].as_str().unwrap();
        let session = harness.session_handle(id);
        assert_eq!(session.status().await.unwrap()["sessionId"], id);
        // Both peers intentionally use the same request ID.
        let (left,right) = tokio::join!(first.request(json!(3), "fx/turn/status", json!({"sessionId":id})), second.request(json!(3), "fx/turn/status", json!({"sessionId":id})));
        assert_eq!(left["result"]["sessionId"], id);
        assert_eq!(right["result"]["sessionId"], id);
        drop(first);
        assert_eq!(session.prompt("SDK continues after ACP disconnect").await.unwrap().stop_reason, "end_turn");
        let response = second.request(json!(4), "session/prompt", json!({"sessionId":id,"prompt":[{"type":"text","text":"ACP continues the SDK session"}]})).await;
        assert_eq!(response["result"]["stopReason"], "end_turn");
        {
            let requests = model.requests.lock().unwrap();
            assert!(requests[1].1.to_string().contains("SDK continues after ACP disconnect"));
        }
        listener.shutdown().await.unwrap();
        assert_eq!(session.status().await.unwrap()["state"], "idle");
        harness.shutdown().await.unwrap();
    }).await.expect("shared ACP test timed out");
}

#[tokio::test]
async fn listener_refuses_non_loopback_bind() {
    let root = tempfile::tempdir().unwrap();
    let harness = Harness::builder(root.path()).build().await.unwrap();
    assert!(harness.listen("0.0.0.0:0".parse().unwrap()).await.is_err());
    harness.shutdown().await.unwrap();
}

struct ApprovedTool;
impl harnel::tool::Tool for ApprovedTool {
    fn spec(&self) -> harnel::tool::ToolSpec {
        harnel::tool::ToolSpec {
            name: "approved_action".into(),
            description: "Perform an action after host approval".into(),
            parameters: json!({"type":"object"}),
            read_only: false,
        }
    }
    fn execute(
        &self,
        _: harnel::tool::ToolCall,
    ) -> harnel::tool::BoxFuture<'_, harnel::Result<harnel::tool::ToolOutput>> {
        Box::pin(async { Ok(harnel::tool::ToolOutput::text("approved action completed")) })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn acp_permission_returns_to_turn_origin_and_rust_tool_stays_in_host() {
    tokio::time::timeout(Duration::from_secs(30), async {
        let model = support::Model::start(|n, body| {
            if n == 0 {
                support::tool("approved_action", json!({}))
            } else {
                assert!(body["input"].to_string().contains("approved action completed"));
                support::text("approved")
            }
        }).await;
        let root = tempfile::tempdir().unwrap();
        let harness = Harness::builder(root.path()).native_tools(false).tool(ApprovedTool)
            .env("FX_PERMISSION_MODE", "ask").model("openai/gpt-5")
            .base_url(&model.url).api_key("fixture-key").build().await.unwrap();
        let session = harness.session().await.unwrap();
        let listener = harness.listen("127.0.0.1:0".parse().unwrap()).await.unwrap();
        let mut observer = Client::connect(listener.local_addr()).await;
        let mut origin = Client::connect(listener.local_addr()).await;
        observer.initialize().await;
        origin.initialize().await;
        origin.send(json!({"jsonrpc":"2.0","id":20,"method":"session/prompt","params":{
            "sessionId":session.id(),"prompt":[{"type":"text","text":"Perform approved_action"}]
        }})).await;
        let mut approvals = 0;
        loop {
            let mut line = String::new();
            assert!(origin.read.read_line(&mut line).await.unwrap() > 0);
            let message: Value = serde_json::from_str(&line).unwrap();
            if message["method"] == "session/request_permission" {
                approvals += 1;
                assert_eq!(message["params"]["sessionId"], session.id());
                origin.send(json!({"jsonrpc":"2.0","id":message["id"],"result":{
                    "outcome":{"outcome":"selected","optionId":"allow_once"}
                }})).await;
            } else if message["id"] == 20 {
                assert_eq!(message["result"]["stopReason"], "end_turn");
                break;
            } else {
                assert!(message.get("id").is_none(), "unexpected outbound request: {message}");
            }
        }
        assert_eq!(approvals, 1);
        observer.send(json!({"jsonrpc":"2.0","id":30,"method":"fx/turn/status","params":{"sessionId":session.id()}})).await;
        loop {
            let mut line = String::new();
            assert!(observer.read.read_line(&mut line).await.unwrap() > 0);
            let message: Value = serde_json::from_str(&line).unwrap();
            assert!(message.get("id").is_none() || message["id"] == 30, "request leaked to observer: {message}");
            if message["id"] == 30 { break; }
        }
        listener.shutdown().await.unwrap();
        harness.shutdown().await.unwrap();
    }).await.expect("ACP permission routing timed out");
}

struct PausedTool(std::sync::Arc<tokio::sync::Notify>);
impl harnel::tool::Tool for PausedTool {
    fn spec(&self) -> harnel::tool::ToolSpec {
        harnel::tool::ToolSpec {
            name: "pause_for_cancel".into(),
            description: "Wait until the turn is cancelled".into(),
            parameters: json!({"type":"object"}),
            read_only: true,
        }
    }
    fn execute(
        &self,
        _: harnel::tool::ToolCall,
    ) -> harnel::tool::BoxFuture<'_, harnel::Result<harnel::tool::ToolOutput>> {
        Box::pin(async move {
            self.0.notify_one();
            std::future::pending().await
        })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn acp_cancels_shared_turn_then_sdk_continues_after_disconnect() {
    tokio::time::timeout(Duration::from_secs(30), async {
        let model = support::Model::start(|n, _| {
            if n == 0 { support::tool("pause_for_cancel", json!({})) }
            else { support::text("SDK resumed after ACP cancellation") }
        }).await;
        let started = std::sync::Arc::new(tokio::sync::Notify::new());
        let root = tempfile::tempdir().unwrap();
        let harness = Harness::builder(root.path()).native_tools(false).tool(PausedTool(started.clone()))
            .model("openai/gpt-5").base_url(&model.url).api_key("fixture-key").build().await.unwrap();
        let session = harness.session().await.unwrap();
        let mut events = harness.subscribe();
        let listener = harness.listen("127.0.0.1:0".parse().unwrap()).await.unwrap();
        let mut client = Client::connect(listener.local_addr()).await;
        client.initialize().await;
        client.send(json!({"jsonrpc":"2.0","id":20,"method":"session/prompt","params":{
            "sessionId":session.id(),"prompt":[{"type":"text","text":"Call pause_for_cancel"}]
        }})).await;
        started.notified().await;
        assert_eq!(events.recv().await.unwrap().params["sessionId"], session.id());
        client.send(json!({"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId":session.id()}})).await;
        assert_eq!(client.reply(json!(20)).await["result"]["stopReason"], "cancelled");
        drop(client);
        assert_eq!(session.ask("Continue through the SDK").await.unwrap().text, "SDK resumed after ACP cancellation");
        listener.shutdown().await.unwrap();
        harness.shutdown().await.unwrap();
    }).await.expect("shared cancellation timed out");
}
