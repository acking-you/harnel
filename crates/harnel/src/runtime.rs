use crate::{
    Error, Result, RpcError,
    tool::{ClientHandler, DefaultHandler, Tool},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::sync::{broadcast, oneshot, watch};

pub(crate) const MAX_FRAME: usize = 1024 * 1024;
const MAX_PENDING: usize = 128;
type Pending = HashMap<u64, oneshot::Sender<Result<Value>>>;

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    #[default]
    Gateway,
    Codex,
    Grok,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub sequence: u64,
    pub method: String,
    pub params: Value,
}

pub struct Events {
    receiver: broadcast::Receiver<Event>,
    closed: watch::Receiver<bool>,
}
impl Events {
    /// Returns the next queued event without waiting.
    pub fn try_recv(&mut self) -> Result<Option<Event>> {
        match self.receiver.try_recv() {
            Ok(event) => Ok(Some(event)),
            Err(broadcast::error::TryRecvError::Empty) if !*self.closed.borrow() => Ok(None),
            Err(broadcast::error::TryRecvError::Lagged(n)) => Err(Error::Lagged(n)),
            Err(_) => Err(Error::Closed),
        }
    }
    /// Slow subscribers get an explicit gap error and can continue receiving.
    pub async fn recv(&mut self) -> Result<Event> {
        if let Some(event) = self.try_recv()? {
            return Ok(event);
        }
        let received = tokio::select! {
            event = self.receiver.recv() => Some(event.map_err(|error| match error {
                broadcast::error::RecvError::Closed => Error::Closed,
                broadcast::error::RecvError::Lagged(n) => Error::Lagged(n),
            })),
            _ = self.closed.wait_for(|closed| *closed) => None,
        };
        received.unwrap_or_else(|| self.try_recv()?.ok_or(Error::Closed))
    }
}

pub struct Builder {
    workspace: PathBuf,
    state_dir: Option<PathBuf>,
    model: Option<String>,
    provider: Provider,
    api_key: Option<String>,
    base_url: Option<String>,
    instructions: Option<String>,
    native_tools: bool,
    environment: HashMap<String, String>,
    tools: Vec<Arc<dyn Tool>>,
    handler: Arc<dyn ClientHandler>,
}

impl Builder {
    pub fn new(workspace: impl Into<PathBuf>) -> Self {
        Self {
            workspace: workspace.into(),
            state_dir: None,
            model: None,
            provider: Provider::Gateway,
            api_key: None,
            base_url: None,
            instructions: None,
            native_tools: true,
            environment: HashMap::new(),
            tools: Vec::new(),
            handler: Arc::new(DefaultHandler),
        }
    }
    pub fn state_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.state_dir = Some(path.into());
        self
    }
    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }
    pub fn provider(mut self, provider: Provider) -> Self {
        self.provider = provider;
        self
    }
    pub fn api_key(mut self, key: impl Into<String>) -> Self {
        self.api_key = Some(key.into());
        self
    }
    /// A Responses-compatible base URL; provider credentials remain instance-local.
    pub fn base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = Some(url.into());
        self
    }
    pub fn instructions(mut self, instructions: impl Into<String>) -> Self {
        self.instructions = Some(instructions.into());
        self
    }
    pub fn native_tools(mut self, enabled: bool) -> Self {
        self.native_tools = enabled;
        self
    }
    /// Explicit per-instance environment override. Never changes the host environment.
    pub fn env(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.environment.insert(name.into(), value.into());
        self
    }
    pub fn tool(mut self, tool: impl Tool) -> Self {
        self.tools.push(Arc::new(tool));
        self
    }
    pub fn client_handler(mut self, handler: impl ClientHandler) -> Self {
        self.handler = Arc::new(handler);
        self
    }

    /// Starts a native engine inside this process. Requires a Tokio runtime.
    pub async fn build(self) -> Result<Harness> {
        let workspace = self.workspace.canonicalize()?;
        if !workspace.is_dir() {
            return Err(Error::Invalid("workspace must be a directory".into()));
        }
        let temporary = if self.state_dir.is_none() {
            Some(Arc::new(tempfile::tempdir()?))
        } else {
            None
        };
        let home = self
            .state_dir
            .as_deref()
            .unwrap_or_else(|| temporary.as_ref().unwrap().path());
        std::fs::create_dir_all(home)?;
        let home = home.canonicalize()?;
        let specs: Vec<_> = self.tools.iter().map(|tool| tool.spec()).collect();
        let tools = specs
            .iter()
            .zip(self.tools)
            .map(|(spec, tool)| (spec.name.clone(), tool))
            .collect();
        let config = serde_json::to_vec(&json!({
            "home": home, "workspace_root": workspace, "model": self.model,
            "provider": self.provider, "api_key": self.api_key, "responses_base_url": self.base_url,
            "instructions": self.instructions, "native_tools": self.native_tools, "tools": specs,
            "environment": self.environment.into_iter().map(|(key,value)| json!({"key":key,"value":value})).collect::<Vec<_>>()
        }))?;
        let native = Arc::new(harnel_sys::Runtime::new(&config)?);
        let (events, _) = broadcast::channel(512);
        let (closed, _) = watch::channel(false);
        let (turn_changed, _) = watch::channel(0);
        let reader_temporary = temporary.clone();
        let inner = Arc::new(Inner {
            native: native.clone(),
            pending: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            sequence: AtomicU64::new(1),
            events,
            closed,
            tools,
            handler: self.handler,
            active: Mutex::new(None),
            turn_changed,
            initialized: Mutex::new(Value::Null),
            workspace,
            home,
            _temporary: temporary,
        });
        let weak = Arc::downgrade(&inner);
        let executor = tokio::runtime::Handle::current();
        std::thread::Builder::new()
            .name("harnel-events".into())
            .spawn(move || {
                read_loop(native, weak, executor);
                drop(reader_temporary);
            })?;
        let harness = Harness(inner);
        let mut initialized = harness.request("initialize", json!({"protocolVersion":1,"clientCapabilities":{},"clientInfo":{"name":"harnel","version":env!("CARGO_PKG_VERSION")}})).await?;
        initialized["agentInfo"] =
            json!({"name":"harnel","title":"Harnel","version":env!("CARGO_PKG_VERSION")});
        initialized["_meta"]["harnel"] = json!({"nativeRevision":harnel_sys::revision(),"sharedRuntime":true,"listener":"loopback-tcp"});
        *harness.0.initialized.lock().unwrap() = initialized;
        Ok(harness)
    }
}

struct Active {
    id: u64,
    session_id: String,
    handler: Arc<dyn ClientHandler>,
}

pub(crate) struct Inner {
    native: Arc<harnel_sys::Runtime>,
    pending: Mutex<Pending>,
    next_id: AtomicU64,
    sequence: AtomicU64,
    events: broadcast::Sender<Event>,
    closed: watch::Sender<bool>,
    tools: HashMap<String, Arc<dyn Tool>>,
    handler: Arc<dyn ClientHandler>,
    active: Mutex<Option<Active>>,
    turn_changed: watch::Sender<u64>,
    initialized: Mutex<Value>,
    workspace: PathBuf,
    home: PathBuf,
    _temporary: Option<Arc<tempfile::TempDir>>,
}
impl Drop for Inner {
    fn drop(&mut self) {
        self.native.close();
    }
}

/// Cheap clones share one native engine, loaded session, and provider state.
#[derive(Clone)]
pub struct Harness(pub(crate) Arc<Inner>);
impl Harness {
    pub fn builder(workspace: impl Into<PathBuf>) -> Builder {
        Builder::new(workspace)
    }
    pub fn subscribe(&self) -> Events {
        Events {
            receiver: self.0.events.subscribe(),
            closed: self.0.closed.subscribe(),
        }
    }
    pub fn workspace(&self) -> &Path {
        &self.0.workspace
    }
    pub fn state_dir(&self) -> &Path {
        &self.0.home
    }
    pub fn capabilities(&self) -> Value {
        self.0.initialized.lock().unwrap().clone()
    }
    pub fn native_revision(&self) -> &'static str {
        harnel_sys::revision()
    }
    pub(crate) fn closed(&self) -> watch::Receiver<bool> {
        self.0.closed.subscribe()
    }

    /// Calls any fx ACP control method. Parameters and replies retain the native
    /// protocol shape, so new native capabilities do not require a Rust release.
    /// Dropping a prompt future cancels that turn; other submitted operations
    /// may still finish. Control requests time out after 60 seconds.
    pub async fn request(&self, method: &str, params: Value) -> Result<Value> {
        self.request_from(method, params, None).await
    }

    pub(crate) async fn request_from(
        &self,
        method: &str,
        params: Value,
        handler: Option<Arc<dyn ClientHandler>>,
    ) -> Result<Value> {
        if *self.0.closed.borrow() {
            return Err(Error::Closed);
        }
        let id = self.0.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        {
            let mut pending = self.0.pending.lock().unwrap();
            if pending.len() >= MAX_PENDING {
                return Err(Error::Busy);
            }
            pending.insert(id, tx);
        }
        let mut guard = RequestGuard {
            inner: &self.0,
            id,
            turn: false,
            complete: false,
        };
        if method == "session/prompt" {
            let mut active = self.0.active.lock().unwrap();
            if active.is_some() {
                return Err(Error::Busy);
            }
            *active = Some(Active {
                id,
                session_id: params["sessionId"].as_str().unwrap_or_default().into(),
                handler: handler.unwrap_or_else(|| self.0.handler.clone()),
            });
            self.0.turn_changed.send_replace(id);
            guard.turn = true;
        }
        send(
            &self.0.native,
            &json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}),
        )?;
        let result = if guard.turn {
            rx.await.map_err(|_| Error::Closed)?
        } else {
            tokio::time::timeout(Duration::from_secs(60), rx)
                .await
                .map_err(|_| Error::Timeout)?
                .map_err(|_| Error::Closed)?
        };
        guard.complete = true;
        result
    }

    pub fn notify(&self, method: &str, params: Value) -> Result<()> {
        send(
            &self.0.native,
            &json!({"jsonrpc":"2.0","method":method,"params":params}),
        )
    }

    /// Cancels work and waits for the engine's output channel to finish.
    /// All clones become closed. Native workers are joined when handles drop.
    pub async fn shutdown(&self) -> Result<()> {
        let mut closed = self.0.closed.subscribe();
        self.0.native.close();
        closed
            .wait_for(|done| *done)
            .await
            .map_err(|_| Error::Closed)?;
        if self.0.native.exit_code() == 0 {
            Ok(())
        } else {
            Err(Error::Invalid("native engine exited with an error".into()))
        }
    }
}

struct RequestGuard<'a> {
    inner: &'a Inner,
    id: u64,
    turn: bool,
    complete: bool,
}
impl Drop for RequestGuard<'_> {
    fn drop(&mut self) {
        self.inner.pending.lock().unwrap().remove(&self.id);
        if self.turn {
            let mut active = self.inner.active.lock().unwrap();
            if active.as_ref().is_some_and(|turn| turn.id == self.id) {
                let turn = active.take().unwrap();
                self.inner.turn_changed.send_replace(0);
                if !self.complete {
                    let _ = send(
                        &self.inner.native,
                        &json!({"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId":turn.session_id}}),
                    );
                }
            }
        }
    }
}

fn send(native: &harnel_sys::Runtime, value: &Value) -> Result<()> {
    let mut frame = serde_json::to_vec(value)?;
    if frame.len() > MAX_FRAME {
        return Err(Error::Invalid("ACP frame exceeds 1 MiB".into()));
    }
    frame.push(b'\n');
    native.write(&frame)?;
    Ok(())
}

fn read_loop(
    native: Arc<harnel_sys::Runtime>,
    weak: Weak<Inner>,
    executor: tokio::runtime::Handle,
) {
    let mut buffer = [0; 8192];
    let mut frame = Vec::new();
    'read: loop {
        let count = match native.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        for byte in &buffer[..count] {
            if *byte == b'\n' {
                if frame.is_empty() {
                    continue;
                }
                let message = match serde_json::from_slice::<Value>(&frame) {
                    Ok(value) => value,
                    Err(_) => break 'read,
                };
                frame.clear();
                let Some(inner) = weak.upgrade() else {
                    break 'read;
                };
                dispatch(&inner, message, &executor);
            } else {
                if frame.len() >= MAX_FRAME {
                    break 'read;
                }
                frame.push(*byte);
            }
        }
    }
    native.close();
    if let Some(inner) = weak.upgrade() {
        inner.pending.lock().unwrap().clear();
        inner.closed.send_replace(true);
    }
}

fn dispatch(inner: &Arc<Inner>, mut message: Value, executor: &tokio::runtime::Handle) {
    if let Some(method) = message["method"].as_str().map(str::to_owned) {
        let params = message["params"].take();
        if let Some(id) = message.get("id").cloned() {
            let weak = Arc::downgrade(inner);
            let mut closed = inner.closed.subscribe();
            let mut turn_changed = inner.turn_changed.subscribe();
            let turn_id = *turn_changed.borrow_and_update();
            let handler = inner
                .active
                .lock()
                .unwrap()
                .as_ref()
                .map(|turn| turn.handler.clone())
                .unwrap_or_else(|| inner.handler.clone());
            let tool = params["name"]
                .as_str()
                .and_then(|name| inner.tools.get(name))
                .cloned();
            executor.spawn(async move {
                let work = async move {
                    if method == "_harnel/tool/call" {
                        let tool = tool.ok_or_else(|| Error::Invalid("host tool is not registered".into()))?;
                        let output = tool.execute(serde_json::from_value(params)?).await?;
                        serde_json::to_value(output).map_err(Error::from)
                    } else { handler.request(method, params).await }
                };
                // Isolate host panics while guaranteeing dropped, timed-out,
                // or cancelled callbacks do not leave detached futures running.
                let mut task = AbortTask(tokio::spawn(work));
                let result = tokio::select! {
                    _ = closed.wait_for(|done| *done) => return,
                    _ = turn_changed.wait_for(|current| *current != turn_id), if turn_id != 0 => return,
                    result = tokio::time::timeout(Duration::from_secs(300), &mut task.0) => match result {
                        Ok(Ok(result)) => result,
                        Ok(Err(_)) => Err(Error::Invalid("host callback panicked".into())),
                        Err(_) => Err(Error::Timeout),
                    }
                };
                if let Some(inner) = weak.upgrade() {
                    let reply = match result {
                        Ok(value) => json!({"jsonrpc":"2.0","id":id,"result":value}),
                        Err(error) => json!({"jsonrpc":"2.0","id":id,"error":error.rpc()}),
                    };
                    if let Err(error) = send(&inner.native, &reply) {
                        // An oversized host result must settle the native
                        // waiter instead of leaving the turn parked forever.
                        let _ = send(&inner.native, &json!({"jsonrpc":"2.0","id":id,"error":error.rpc()}));
                    }
                }
            });
        } else {
            let sequence = inner.sequence.fetch_add(1, Ordering::Relaxed);
            let _ = inner.events.send(Event {
                sequence,
                method,
                params,
            });
        }
    } else if let Some(id) = message["id"].as_u64() {
        if let Some(reply) = inner.pending.lock().unwrap().remove(&id) {
            let result = if let Some(error) = message.get_mut("error") {
                serde_json::from_value::<RpcError>(error.take())
                    .map_err(Error::from)
                    .and_then(|error| Err(error.into()))
            } else {
                Ok(message["result"].take())
            };
            let _ = reply.send(result);
        }
    }
}

struct AbortTask<T>(tokio::task::JoinHandle<T>);
impl<T> Drop for AbortTask<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}
