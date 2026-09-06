use harnel::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{
        TcpStream,
        tcp::{OwnedReadHalf, OwnedWriteHalf},
    },
};

pub struct Client {
    pub read: BufReader<OwnedReadHalf>,
    write: OwnedWriteHalf,
}
impl Client {
    pub async fn connect(address: std::net::SocketAddr) -> Self {
        let socket = TcpStream::connect(address).await.unwrap();
        let (read, write) = socket.into_split();
        Self {
            read: BufReader::new(read),
            write,
        }
    }
    pub async fn send(&mut self, message: Value) {
        self.write
            .write_all(format!("{message}\n").as_bytes())
            .await
            .unwrap();
    }
    pub async fn reply(&mut self, id: Value) -> Value {
        loop {
            let mut line = String::new();
            assert!(self.read.read_line(&mut line).await.unwrap() > 0);
            let value: Value = serde_json::from_str(&line).unwrap();
            if value.get("id") == Some(&id) {
                return value;
            }
        }
    }
    pub async fn request(&mut self, id: Value, method: &str, params: Value) -> Value {
        self.send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
            .await;
        self.reply(id).await
    }
    pub async fn initialize(&mut self) {
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
