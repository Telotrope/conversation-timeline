//! A stand-in for DynamoDB that answers every `BatchWriteItem` by handing
//! back some or all of the rows it was sent as unprocessed, as DynamoDB
//! does when it is busy. DynamoDB Local never does this, so without the
//! stand-in the adapters' resend path could not be tested at all.
//!
//! It speaks just enough of DynamoDB's protocol (HTTP POST with a JSON body,
//! the operation named in `X-Amz-Target`) for the real SDK client to talk to
//! it, so tests go through the adapters' public methods unchanged.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use aws_sdk_dynamodb::config::{BehaviorVersion, Credentials, Region};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

/// How the stand-in answers.
#[derive(Clone, Copy)]
pub enum Busy {
    /// Every row of every batch comes back unprocessed.
    Always,
    /// The first `n` answers hand back every row; later ones take them all.
    FirstAnswers(usize),
}

/// A running stand-in and how many `BatchWriteItem` calls it has answered.
pub struct FakeDynamo {
    pub client: aws_sdk_dynamodb::Client,
    pub calls: Arc<AtomicUsize>,
}

pub async fn start(busy: Busy) -> FakeDynamo {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind a local port");
    let addr: SocketAddr = listener.local_addr().expect("read the bound address");
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    tokio::spawn(async move {
        loop {
            let Ok((socket, _)) = listener.accept().await else {
                return;
            };
            let counter = counter.clone();
            tokio::spawn(async move { serve(socket, busy, counter).await });
        }
    });
    let config = aws_sdk_dynamodb::Config::builder()
        .behavior_version(BehaviorVersion::latest())
        .credentials_provider(Credentials::new("test", "test", None, None, "test"))
        .region(Region::new("us-east-1"))
        .endpoint_url(format!("http://{addr}"))
        .build();
    FakeDynamo {
        client: aws_sdk_dynamodb::Client::from_conf(config),
        calls,
    }
}

/// Answers requests on one connection until the client closes it.
async fn serve(socket: tokio::net::TcpStream, busy: Busy, calls: Arc<AtomicUsize>) {
    let (read, mut write) = socket.into_split();
    let mut reader = BufReader::new(read);
    loop {
        let mut length = 0usize;
        let mut line = String::new();
        // Request line and headers, up to the blank line.
        loop {
            line.clear();
            match reader.read_line(&mut line).await {
                Ok(0) | Err(_) => return,
                Ok(_) => {}
            }
            if line == "\r\n" {
                break;
            }
            if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                length = value.trim().parse().unwrap_or(0);
            }
        }
        let mut body = vec![0u8; length];
        if reader.read_exact(&mut body).await.is_err() {
            return;
        }
        let request: serde_json::Value = serde_json::from_slice(&body).expect("a JSON request");
        let call = calls.fetch_add(1, Ordering::SeqCst);
        let hand_back = match busy {
            Busy::Always => true,
            Busy::FirstAnswers(n) => call < n,
        };
        let answer = if hand_back {
            serde_json::json!({ "UnprocessedItems": request["RequestItems"] })
        } else {
            serde_json::json!({ "UnprocessedItems": {} })
        }
        .to_string();
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/x-amz-json-1.0\r\ncontent-length: {}\r\n\r\n{answer}",
            answer.len()
        );
        if write.write_all(response.as_bytes()).await.is_err() {
            return;
        }
    }
}
