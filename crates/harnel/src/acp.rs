//! ACP attachments to an existing harness. Listener framing is the same
//! newline-delimited JSON-RPC used by stdio, carried over loopback TCP.
//! The socket is a trusted local control interface, including tool execution.
use crate::{
    Error, Harness, Result, RpcError,
    runtime::MAX_FRAME,
    tool::{BoxFuture, ClientHandler},
};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    sync::{mpsc, oneshot},
    task::JoinSet,
};

struct Peer {
    output: mpsc::Sender<Value>,
    replies: Mutex<HashMap<u64, oneshot::Sender<Result<Value>>>>,
    next_id: AtomicU64,
}
impl ClientHandler for Peer {
    fn request(&self, method: String, params: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move {
            let id = self.next_id.fetch_add(1, Ordering::Relaxed);
            let (tx, rx) = oneshot::channel();
            {
                let mut replies = self.replies.lock().unwrap();
                if replies.len() >= 32 {
                    return Err(Error::Busy);
                }
                replies.insert(id, tx);
            }
            let _guard = ReplyGuard { peer: self, id };
            self.output
                .send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
                .await
                .map_err(|_| Error::Closed)?;
            rx.await.map_err(|_| Error::Closed)?
        })
    }
}
struct ReplyGuard<'a> {
    peer: &'a Peer,
    id: u64,
}
impl Drop for ReplyGuard<'_> {
    fn drop(&mut self) {
        self.peer.replies.lock().unwrap().remove(&self.id);
    }
}

/// Reads a bounded frame without allocating an unbounded `read_line` buffer.
async fn frame(reader: &mut (impl AsyncBufRead + Unpin)) -> Result<Option<Value>> {
    let mut data = Vec::new();
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            return if data.is_empty() {
                Ok(None)
            } else {
                Err(Error::Invalid("truncated ACP frame".into()))
            };
        }
        let end = available.iter().position(|byte| *byte == b'\n');
        let count = end.map_or(available.len(), |index| index + 1);
        if data.len() + count > MAX_FRAME + 1 {
            return Err(Error::Invalid("ACP frame exceeds 1 MiB".into()));
        }
        data.extend_from_slice(&available[..count]);
        reader.consume(count);
        if end.is_some() {
            if data.iter().all(u8::is_ascii_whitespace) {
                data.clear();
                continue;
            }
            return Ok(Some(serde_json::from_slice(&data)?));
        }
    }
}

impl Harness {
    /// Attach a byte stream. Disconnecting cancels that peer's pending prompts,
    /// while saved sessions, provider state, and other attachments remain live.
    pub async fn serve_acp<R, W>(&self, read: R, mut write: W) -> Result<()>
    where
        R: AsyncRead + Unpin + Send,
        W: AsyncWrite + Unpin + Send,
    {
        let (output, mut outgoing) = mpsc::channel::<Value>(128);
        let peer = Arc::new(Peer {
            output,
            replies: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
        });
        let mut events = self.subscribe();
        let mut reader = BufReader::new(read);
        let mut tasks = JoinSet::new();
        let mut closed = self.closed();
        let read_loop = async {
            let mut initialized = false;
            while let Some(mut message) = frame(&mut reader).await? {
                while tasks.try_join_next().is_some() {}
                if message["jsonrpc"] != "2.0" {
                    return Err(Error::Invalid("expected JSON-RPC 2.0".into()));
                }
                let id = message.get("id").cloned();
                if let Some(method) = message["method"].as_str().map(str::to_owned) {
                    if method.starts_with("_harnel/") {
                        return Err(Error::Invalid("private control method".into()));
                    }
                    let params = message["params"].take();
                    if let Some(id) = id {
                        if !(id.is_string() || id.is_i64()) {
                            return Err(Error::Invalid("invalid request ID".into()));
                        }
                        if method == "initialize" {
                            let result = if initialized {
                                Err(Error::Invalid("already initialized".into()))
                            } else if params["protocolVersion"] != 1 {
                                Err(Error::Invalid("protocolVersion must be 1".into()))
                            } else {
                                initialized = true;
                                Ok(self.capabilities())
                            };
                            peer.output
                                .send(response(id, result))
                                .await
                                .map_err(|_| Error::Closed)?;
                            continue;
                        }
                        if !initialized {
                            peer.output
                                .send(response(id, Err(Error::Invalid("initialize first".into()))))
                                .await
                                .map_err(|_| Error::Closed)?;
                            continue;
                        }
                        if tasks.len() >= 64 {
                            peer.output
                                .send(response(id, Err(Error::Busy)))
                                .await
                                .map_err(|_| Error::Closed)?;
                            continue;
                        }
                        let harness = self.clone();
                        let peer = peer.clone();
                        tasks.spawn(async move {
                            let result = harness
                                .request_from(&method, params, Some(peer.clone()))
                                .await;
                            let _ = peer.output.send(response(id, result)).await;
                        });
                    } else if initialized {
                        self.notify(&method, params)?;
                    }
                } else if let Some(id) = id.and_then(|value| value.as_u64()) {
                    let tx = peer.replies.lock().unwrap().remove(&id);
                    if let Some(tx) = tx {
                        let result = if message.get("error").is_some() {
                            Err(Error::Rpc(serde_json::from_value::<RpcError>(
                                message["error"].take(),
                            )?))
                        } else {
                            Ok(message["result"].take())
                        };
                        let _ = tx.send(result);
                    }
                } else {
                    return Err(Error::Invalid("invalid ACP message".into()));
                }
            }
            Ok(())
        };
        let write_loop = async {
            loop {
                let message = tokio::select! {
                    response = outgoing.recv() => response.ok_or(Error::Closed)?,
                    event = events.recv() => {
                        let event = event?;
                        json!({"jsonrpc":"2.0","method":event.method,"params":event.params})
                    }
                };
                let mut bytes = serde_json::to_vec(&message)?;
                if bytes.len() > MAX_FRAME {
                    return Err(Error::Invalid("ACP output exceeds 1 MiB".into()));
                }
                bytes.push(b'\n');
                write.write_all(&bytes).await?;
                write.flush().await?;
            }
        };
        let result = tokio::select! {
            result = read_loop => result,
            result = write_loop => result,
            _ = closed.wait_for(|done| *done) => Ok(()),
        };
        tasks.abort_all();
        while tasks.join_next().await.is_some() {}
        peer.replies.lock().unwrap().clear();
        result
    }

    pub async fn serve_stdio(&self) -> Result<()> {
        self.serve_acp(tokio::io::stdin(), tokio::io::stdout())
            .await
    }

    /// Bind a trusted loopback listener. Use port 0 for an OS-selected port.
    pub async fn listen(&self, address: SocketAddr) -> Result<Listener> {
        if !address.ip().is_loopback() {
            return Err(Error::Invalid(
                "ACP listener must bind a loopback address".into(),
            ));
        }
        let listener = TcpListener::bind(address).await?;
        let address = listener.local_addr()?;
        let harness = self.clone();
        let (stop, mut stopping) = oneshot::channel();
        let task = tokio::spawn(async move {
            let mut connections = JoinSet::new();
            let mut closed = harness.closed();
            loop {
                tokio::select! {
                    _ = &mut stopping => break,
                    _ = closed.wait_for(|done| *done) => break,
                    _ = connections.join_next(), if !connections.is_empty() => {},
                    accepted = listener.accept() => {
                        let (socket, _) = accepted?;
                        if connections.len() >= 32 { drop(socket); continue; }
                        socket.set_nodelay(true)?;
                        let harness = harness.clone();
                        connections.spawn(async move {
                            let (read, write) = socket.into_split();
                            harness.serve_acp(read, write).await
                        });
                    }
                }
            }
            connections.abort_all();
            while connections.join_next().await.is_some() {}
            Ok(())
        });
        Ok(Listener {
            address,
            stop: Some(stop),
            task: Some(task),
        })
    }
}

fn response(id: Value, result: Result<Value>) -> Value {
    match result {
        Ok(result) => json!({"jsonrpc":"2.0","id":id,"result":result}),
        Err(error) => json!({"jsonrpc":"2.0","id":id,"error":error.rpc()}),
    }
}

pub struct Listener {
    address: SocketAddr,
    stop: Option<oneshot::Sender<()>>,
    task: Option<tokio::task::JoinHandle<Result<()>>>,
}
impl Listener {
    pub fn local_addr(&self) -> SocketAddr {
        self.address
    }
    pub async fn shutdown(mut self) -> Result<()> {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        self.task
            .take()
            .unwrap()
            .await
            .map_err(|error| Error::Invalid(error.to_string()))?
    }
}
impl Drop for Listener {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }
}

/// Connect an ACP stdio client to an already running Harnel listener.
pub async fn bridge_stdio(address: SocketAddr) -> Result<()> {
    if !address.ip().is_loopback() {
        return Err(Error::Invalid("bridge target must be loopback".into()));
    }
    let socket = TcpStream::connect(address).await?;
    let (mut read, mut write) = socket.into_split();
    let mut stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();
    tokio::select! {
        result = tokio::io::copy(&mut stdin, &mut write) => { result?; },
        result = tokio::io::copy(&mut read, &mut stdout) => { result?; },
    }
    Ok(())
}
