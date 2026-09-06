#![allow(dead_code)]
pub mod acp;
use harnel::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    task::JoinHandle,
};

pub struct Model {
    pub url: String,
    pub requests: Arc<Mutex<Vec<(String, Value)>>>,
    task: JoinHandle<()>,
}
impl Model {
    pub async fn start(
        reply: impl Fn(usize, &Value) -> Vec<Value> + Send + Sync + 'static,
    ) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded = requests.clone();
        let reply = Arc::new(reply);
        let count = Arc::new(AtomicUsize::new(0));
        let task = tokio::spawn(async move {
            let mut connections = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    _ = connections.join_next(), if !connections.is_empty() => {},
                    socket = listener.accept() => {
                        let (mut socket, _) = socket.unwrap();
                        let requests = recorded.clone();
                        let reply = reply.clone();
                        let count = count.clone();
                        connections.spawn(async move {
                            let mut bytes = Vec::new();
                            let mut buffer = [0; 4096];
                            let header_end = loop {
                                let n = socket.read(&mut buffer).await.unwrap();
                                if n == 0 { return; }
                                bytes.extend_from_slice(&buffer[..n]);
                                assert!(bytes.len() < 8 * 1024 * 1024);
                                if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") { break end + 4; }
                            };
                            let headers = String::from_utf8_lossy(&bytes[..header_end]).into_owned();
                            let length = headers.lines().find_map(|line| line.to_ascii_lowercase().strip_prefix("content-length:").map(|n| n.trim().parse::<usize>().unwrap())).unwrap_or(0);
                            while bytes.len() < header_end + length {
                                let n = socket.read(&mut buffer).await.unwrap();
                                if n == 0 { return; }
                                bytes.extend_from_slice(&buffer[..n]);
                            }
                            let (kind, body) = if headers.starts_with("GET ") {
                                ("application/json", json!({"data":[{"id":"gpt-5","object":"model","owned_by":"openai"}]}).to_string())
                            } else {
                                let body: Value = serde_json::from_slice(&bytes[header_end..header_end+length]).unwrap();
                                let events = reply(count.fetch_add(1, Ordering::SeqCst), &body);
                                requests.lock().unwrap().push((headers, body));
                                ("text/event-stream", events.into_iter().map(|event| format!("data: {event}\n\n")).collect::<String>() + "data: [DONE]\n\n")
                            };
                            let response = format!("HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                            let _ = socket.write_all(response.as_bytes()).await;
                        });
                    }
                }
            }
        });
        Self {
            url,
            requests,
            task,
        }
    }
}
impl Drop for Model {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub fn completed() -> Value {
    json!({"type":"response.completed","response":{"status":"completed","usage":{"input_tokens":3,"output_tokens":5,"total_tokens":8}}})
}
pub fn text(value: &str) -> Vec<Value> {
    vec![
        json!({"type":"response.output_text.delta","item_id":"answer_1","output_index":0,"content_index":0,"delta":value}),
        completed(),
    ]
}
pub fn tool(name: &str, arguments: Value) -> Vec<Value> {
    vec![
        json!({"type":"response.output_item.added","output_index":0,"item":{"type":"function_call","id":"item_1","call_id":"call_1","name":name}}),
        json!({"type":"response.function_call_arguments.done","item_id":"item_1","call_id":"call_1","output_index":0,"arguments":arguments.to_string()}),
        completed(),
    ]
}
