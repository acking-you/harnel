mod support;
use harnel::{
    Harness, Result, json,
    tool::{BoxFuture, Tool, ToolCall, ToolOutput, ToolSpec},
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::sync::Notify;

struct BlockingTool {
    started: Arc<Notify>,
    dropped: Arc<AtomicBool>,
}
struct DropMarker(Arc<AtomicBool>);
impl Drop for DropMarker {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}
impl Tool for BlockingTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "wait_for_host".into(),
            description: "Wait for an application event".into(),
            parameters: json!({"type":"object"}),
            read_only: true,
        }
    }
    fn execute(&self, _: ToolCall) -> BoxFuture<'_, Result<ToolOutput>> {
        Box::pin(async move {
            let _marker = DropMarker(self.dropped.clone());
            self.started.notify_one();
            std::future::pending().await
        })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancelling_a_turn_drops_the_host_future_and_keeps_session_usable() {
    tokio::time::timeout(Duration::from_secs(30), async {
        let model = support::Model::start(|n, _| {
            if n == 0 {
                support::tool("wait_for_host", json!({}))
            } else {
                support::text("resumed")
            }
        })
        .await;
        let root = tempfile::tempdir().unwrap();
        let started = Arc::new(Notify::new());
        let dropped = Arc::new(AtomicBool::new(false));
        let harness = Harness::builder(root.path())
            .native_tools(false)
            .model("openai/gpt-5")
            .base_url(&model.url)
            .api_key("fixture-key")
            .tool(BlockingTool {
                started: started.clone(),
                dropped: dropped.clone(),
            })
            .build()
            .await
            .unwrap();
        let session = harness.session().await.unwrap();
        let other = session.clone();
        let prompt = tokio::spawn(async move { other.prompt("Wait for the host").await });
        started.notified().await;
        session.cancel().unwrap();
        assert_eq!(prompt.await.unwrap().unwrap().stop_reason, "cancelled");
        while !dropped.load(Ordering::SeqCst) {
            tokio::task::yield_now().await;
        }
        assert_eq!(
            session.prompt("Continue").await.unwrap().stop_reason,
            "end_turn"
        );
        let mut events = harness.subscribe();
        harness.shutdown().await.unwrap();
        while events.recv().await.is_ok() {}
    })
    .await
    .expect("cancellation timed out");
}
