use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

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

fn first_argument(request: &[u8]) -> Option<&[u8]> {
    let first_end = request.windows(2).position(|window| window == b"\r\n")?;
    let count = std::str::from_utf8(&request[1..first_end])
        .ok()?
        .parse::<usize>()
        .ok()?;
    if count == 0 {
        return None;
    }
    let start = first_end + 2;
    let length_end = request[start..]
        .windows(2)
        .position(|window| window == b"\r\n")?
        + start;
    let length = std::str::from_utf8(&request[start + 1..length_end])
        .ok()?
        .parse::<usize>()
        .ok()?;
    let value_start = length_end + 2;
    Some(&request[value_start..value_start + length])
}

async fn serve_connection(
    mut stream: TcpStream,
    scan_requests: Arc<AtomicUsize>,
    delete_requests: Arc<AtomicUsize>,
) {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 512];
    loop {
        let count = stream.read(&mut chunk).await.unwrap();
        if count == 0 {
            return;
        }
        buffer.extend_from_slice(&chunk[..count]);
        while let Some(end) = request_end(&buffer) {
            let request = buffer.drain(..end).collect::<Vec<_>>();
            let Some(command) = first_argument(&request) else {
                continue;
            };
            if command.eq_ignore_ascii_case(b"SCAN") {
                if scan_requests.fetch_add(1, Ordering::SeqCst) == 0 {
                    return;
                }
                stream
                    .write_all(b"*2\r\n$1\r\n0\r\n*1\r\n$5\r\nhello\r\n")
                    .await
                    .unwrap();
            } else if command.eq_ignore_ascii_case(b"PING") {
                stream.write_all(b"+PONG\r\n").await.unwrap();
            } else if command.eq_ignore_ascii_case(b"DEL") {
                delete_requests.fetch_add(1, Ordering::SeqCst);
                return;
            } else {
                stream.write_all(b"+OK\r\n").await.unwrap();
            }
        }
    }
}

async fn start_server() -> (std::net::SocketAddr, Arc<AtomicUsize>, Arc<AtomicUsize>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let address = listener.local_addr().unwrap();
    let scan_requests = Arc::new(AtomicUsize::new(0));
    let delete_requests = Arc::new(AtomicUsize::new(0));
    let requests = Arc::clone(&scan_requests);
    let deletes = Arc::clone(&delete_requests);
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(serve_connection(
                stream,
                Arc::clone(&requests),
                Arc::clone(&deletes),
            ));
        }
    });
    (address, scan_requests, delete_requests)
}

#[tokio::test]
async fn redis_scan_recovers_after_the_server_drops_the_socket() {
    let (address, scan_requests, _) = start_server().await;
    let mut imported = lazydb::profile::import_connection_url(
        &format!("redis://{address}/0"),
        Some("redis-reconnect-test"),
    )
    .unwrap();
    imported.profile.id = uuid::Uuid::new_v4();
    let adapter = lazydb::db::redis::RedisAdapter::connect(&imported.profile, None)
        .await
        .unwrap();

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        adapter.scan_keys(0, b"*", 10),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(result.1, vec![b"hello".to_vec()]);
    assert_eq!(scan_requests.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn cloned_adapters_share_the_recovered_connection() {
    let (address, scan_requests, _) = start_server().await;
    let imported = lazydb::profile::import_connection_url(
        &format!("redis://{address}/2"),
        Some("redis-reconnect-clone-test"),
    )
    .unwrap();
    let adapter = lazydb::db::redis::RedisAdapter::connect(&imported.profile, None)
        .await
        .unwrap();
    assert_eq!(adapter.database(), 2);

    let first = adapter.clone();
    let second = adapter.clone();
    let (left, right) = tokio::join!(
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            first.scan_keys(0, b"*", 10)
        ),
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            second.scan_keys(0, b"*", 10)
        ),
    );
    assert_eq!(left.unwrap().unwrap().1, vec![b"hello".to_vec()]);
    assert_eq!(right.unwrap().unwrap().1, vec![b"hello".to_vec()]);
    assert!(scan_requests.load(Ordering::SeqCst) >= 3);
}

#[tokio::test]
async fn dropped_write_response_is_not_replayed() {
    let (address, _, delete_requests) = start_server().await;
    let imported = lazydb::profile::import_connection_url(
        &format!("redis://{address}/0"),
        Some("redis-reconnect-write-test"),
    )
    .unwrap();
    let adapter = lazydb::db::redis::RedisAdapter::connect(&imported.profile, None)
        .await
        .unwrap();

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        adapter.delete_key(b"write-once"),
    )
    .await
    .unwrap();
    assert!(result.is_err());
    assert_eq!(delete_requests.load(Ordering::SeqCst), 1);
}
