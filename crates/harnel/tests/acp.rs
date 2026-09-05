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
