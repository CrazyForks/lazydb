use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

fn request_end(buffer: &[u8]) -> Option<usize> {
    if buffer.first().copied()? != b'*' {
        return None;
    }
    let line_end = buffer.windows(2).position(|window| window == b"\r\n")?;
    let count = std::str::from_utf8(&buffer[1..line_end])
        .ok()?
        .parse::<usize>()
        .ok()?;
    let mut position = line_end + 2;
    for _ in 0..count {
        if buffer.get(position).copied()? != b'$' {
            return None;
        }
        let length_end = buffer[position..]
            .windows(2)
            .position(|window| window == b"\r\n")?
            + position;
        let length = std::str::from_utf8(&buffer[position + 1..length_end])
            .ok()?
            .parse::<usize>()
            .ok()?;
        position = length_end + 2 + length + 2;
        if buffer.len() < position {
            return None;
        }
    }
    Some(position)
}

fn first_command(request: &[u8]) -> Option<&[u8]> {
    let first_end = request.windows(2).position(|window| window == b"\r\n")?;
    let length = std::str::from_utf8(&request[1..first_end])
        .ok()?
        .parse::<usize>()
        .ok()?;
    let start = first_end + 2;
    let end = start.checked_add(length)?;
    let command = &request[start..end];
    Some(command)
}

async fn start_server(response_for: &'static str, hold_response: bool) -> std::net::SocketAddr {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buffer = Vec::new();
        let mut chunk = [0_u8; 256];
        let mut setup_responses = 0;
        loop {
            let count = stream.read(&mut chunk).await.unwrap();
            if count == 0 {
                return;
            }
            buffer.extend_from_slice(&chunk[..count]);
            while let Some(end) = request_end(&buffer) {
                let request = buffer.drain(..end).collect::<Vec<_>>();
                let command = first_command(&request).unwrap_or_default();
                if response_for == "ping" && !command.eq_ignore_ascii_case(b"CLIENT") {
                    stream.write_all(b"+PONG\r\n").await.unwrap();
                } else if command.eq_ignore_ascii_case(b"GET") && response_for == "get" {
                    stream.write_all(b"$5\r\nhello\r\n").await.unwrap();
                } else if response_for == "timeout" && setup_responses >= 2 {
                    if hold_response {
                        tokio::time::sleep(Duration::from_secs(30)).await;
                    }
                } else {
                    stream.write_all(b"+OK\r\n").await.unwrap();
                    setup_responses += 1;
                }
            }
        }
    });
    address
}

#[tokio::test]
async fn redis_client_can_use_tokio_and_decode_a_bounded_reply() {
    let address = start_server("ping", false).await;
    let client = redis::Client::open(format!("redis://{address}/")).unwrap();
    let mut connection = client.get_multiplexed_async_connection().await.unwrap();
    let response: String = redis::cmd("PING")
        .query_async(&mut connection)
        .await
        .unwrap();
    assert_eq!(response, "PONG");
}

#[tokio::test]
async fn dropping_a_response_future_does_not_claim_server_cancellation() {
    let address = start_server("get", true).await;
    let client = redis::Client::open(format!("redis://{address}/")).unwrap();
    let mut connection = client.get_multiplexed_async_connection().await.unwrap();
    let request = tokio::spawn(async move {
        let _: Vec<u8> = redis::cmd("GET")
            .query_async(&mut connection)
            .await
            .unwrap();
    });
    request.abort();
    assert!(request.await.unwrap_err().is_cancelled());
}

#[tokio::test]
async fn response_timeout_is_configurable() {
    let address = start_server("timeout", true).await;
    let client = redis::Client::open(format!("redis://{address}/")).unwrap();
    let config = redis::AsyncConnectionConfig::new()
        .set_connection_timeout(Some(Duration::from_secs(1)))
        .set_response_timeout(Some(Duration::from_millis(10)));
    let mut connection = client
        .get_multiplexed_async_connection_with_config(&config)
        .await
        .unwrap();
    let result: redis::RedisResult<String> = redis::cmd("PING").query_async(&mut connection).await;
    assert!(result.is_err());
}
