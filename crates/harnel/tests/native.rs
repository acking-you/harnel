use harnel::{Harness, json};
use std::time::Duration;
mod support;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn native_lifecycle_and_shared_control() {
    tokio::time::timeout(Duration::from_secs(30), async {
        let workspace = tempfile::tempdir().unwrap();
        let first = Harness::builder(workspace.path())
            .native_tools(false)
            .build()
            .await
            .unwrap();
        let second = Harness::builder(workspace.path())
            .native_tools(false)
            .build()
            .await
            .unwrap();
        assert_ne!(first.state_dir(), second.state_dir());
        assert_eq!(first.capabilities()["protocolVersion"], 1);
        assert_eq!(first.bash_first(true).await.unwrap()["bashFirst"], true);
        assert_eq!(
            first
                .clone()
                .request("fx/toolMode/set", json!({"mode":"standard"}))
                .await
                .unwrap()["bashFirst"],
            false
        );
        assert!(first.request("does/not/exist", json!({})).await.is_err());
        second.shutdown().await.unwrap();
        first.shutdown().await.unwrap();
        assert!(first.list_sessions().await.is_err());
    })
    .await
    .expect("native lifecycle timed out");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn native_turn_streams_and_isolates_provider_credentials() {
    tokio::time::timeout(Duration::from_secs(30), async {
        let model_a = support::Model::start(|_, _| support::text("answer A")).await;
        let model_b = support::Model::start(|_, _| support::text("answer B")).await;
        let workspace = tempfile::tempdir().unwrap();
        let a = Harness::builder(workspace.path())
            .native_tools(false)
            .model("openai/gpt-5")
            .api_key("key-a")
            .base_url(&model_a.url)
            .build()
            .await
            .unwrap();
        let b = Harness::builder(workspace.path())
            .native_tools(false)
            .model("openai/gpt-5")
            .api_key("key-b")
            .base_url(&model_b.url)
            .build()
            .await
            .unwrap();
        let mut events = a.subscribe();
        let session_a = a.session().await.unwrap();
        let session_b = b.session().await.unwrap();
        let (first, second) = tokio::join!(session_a.ask("hello A"), session_b.prompt("hello B"));
        let answer = first.unwrap();
        assert_eq!(answer.text, "answer A");
        assert_eq!(answer.turn.stop_reason, "end_turn");
        assert_eq!(second.unwrap().stop_reason, "end_turn");
        loop {
            let event = events.recv().await.unwrap();
            if event.params.to_string().contains("answer A") {
                break;
            }
        }
        {
            let first = model_a.requests.lock().unwrap();
            assert_eq!(first.len(), 1);
            assert!(first[0].0.contains("Bearer key-a"));
            assert!(!first[0].0.contains("key-b"));
        }
        assert!(
            model_b.requests.lock().unwrap()[0]
                .0
                .contains("Bearer key-b")
        );
        a.shutdown().await.unwrap();
        b.shutdown().await.unwrap();
    })
    .await
    .expect("native model turn timed out");
}
