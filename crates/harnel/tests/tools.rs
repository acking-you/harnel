mod support;
use harnel::{
    Harness, Result, json,
    tool::{BoxFuture, Tool, ToolCall, ToolOutput, ToolSpec},
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

struct Lookup(Arc<AtomicUsize>);
impl Tool for Lookup {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "lookup_inventory".into(),
            description: "Read inventory by SKU".into(),
            parameters: json!({"type":"object","properties":{"sku":{"type":"string"}},"required":["sku"],"additionalProperties":false}),
            read_only: true,
        }
    }
    fn execute(&self, call: ToolCall) -> BoxFuture<'_, Result<ToolOutput>> {
        Box::pin(async move {
            assert_eq!(call.arguments["sku"], "demo");
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(ToolOutput::text("inventory=7"))
        })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rust_tool_runs_inside_native_model_loop() {
    tokio::time::timeout(Duration::from_secs(30), async {
        let model = support::Model::start(|n, body| {
            if n == 0 {
                assert!(
                    body["tools"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|tool| tool["name"] == "lookup_inventory")
                );
                support::tool("lookup_inventory", json!({"sku":"demo"}))
            } else {
                assert!(
                    body["input"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|item| item["type"] == "function_call_output"
                            && item["output"].to_string().contains("inventory=7"))
                );
                support::text("Seven items are available.")
            }
        })
        .await;
        let count = Arc::new(AtomicUsize::new(0));
        let workspace = tempfile::tempdir().unwrap();
        let harness = Harness::builder(workspace.path())
            .native_tools(false)
            .tool(Lookup(count.clone()))
            .model("openai/gpt-5")
            .base_url(&model.url)
            .api_key("test-key")
            .build()
            .await
            .unwrap();
        let session = harness.session().await.unwrap();
        let response = session.prompt("Check inventory for demo").await.unwrap();
        assert_eq!(response.stop_reason, "end_turn");
        assert_eq!(count.load(Ordering::SeqCst), 1);
        assert_eq!(model.requests.lock().unwrap().len(), 2);
        harness.shutdown().await.unwrap();
    })
    .await
    .expect("host tool turn timed out");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn native_shell_executes_without_an_fx_process() {
    tokio::time::timeout(Duration::from_secs(30), async {
        let model = support::Model::start(|n, body| {
            if body["tools"].to_string().contains("permission_decision") {
                support::tool(
                    "permission_decision",
                    json!({"risk":"low","decision":"clear","rationale":"explicit test request"}),
                )
            } else if n == 0 {
                support::tool(
                    "exec_command",
                    json!({"cmd":"printf harnel-native-shell","login":false}),
                )
            } else {
                assert!(
                    body["input"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|item| item["type"] == "function_call_output"
                            && item["output"].to_string().contains("harnel-native-shell")),
                    "missing shell output: {}",
                    body["input"]
                );
                support::text("Shell completed")
            }
        })
        .await;
        let workspace = tempfile::tempdir().unwrap();
        let harness = Harness::builder(workspace.path())
            .model("openai/gpt-5")
            .base_url(&model.url)
            .api_key("test-key")
            .build()
            .await
            .unwrap();
        let session = harness.session().await.unwrap();
        session
            .prompt("Print harnel-native-shell with printf")
            .await
            .unwrap();
        assert_eq!(
            model
                .requests
                .lock()
                .unwrap()
                .iter()
                .filter(|(_, body)| !body["tools"].to_string().contains("permission_decision"))
                .count(),
            2
        );
        harness.shutdown().await.unwrap();
    })
    .await
    .expect("native shell turn timed out");
}
