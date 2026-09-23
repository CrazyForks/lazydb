use std::{
    io::{Read, Write},
    net::TcpListener,
    process::{Command, Stdio},
    thread,
    time::Duration,
};

use lazydb::update::{SystemUpdateHttpClient, UpdateHttpClient};

const PROXY_WORKER: &str = "LAZYDB_UPDATE_HTTP_PROXY_WORKER";

fn clear_proxy_environment(command: &mut Command) {
    for name in [
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "NO_PROXY",
        "http_proxy",
        "https_proxy",
        "all_proxy",
        "no_proxy",
    ] {
        command.env_remove(name);
    }
}

fn run_proxy_worker(name: &str, proxy_name: &str, proxy_url: &str) {
    let mut command = Command::new(std::env::current_exe().expect("integration test executable"));
    clear_proxy_environment(&mut command);
    let output = command
        .arg("--exact")
        .arg(name)
        .arg("--nocapture")
        .env(PROXY_WORKER, name)
        .env(proxy_name, proxy_url)
        .env_remove("NO_PROXY")
        .env_remove("no_proxy")
        .env_remove("ALL_PROXY")
        .env_remove("all_proxy")
        .env_remove(if proxy_name == "HTTP_PROXY" {
            "http_proxy"
        } else {
            "HTTP_PROXY"
        })
        .env_remove(if proxy_name == "HTTPS_PROXY" {
            "https_proxy"
        } else {
            "HTTPS_PROXY"
        })
        .output()
        .expect("run isolated proxy worker");
    assert!(
        output.status.success(),
        "worker failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn serve_proxy_response(expected_prefix: &'static str, response: &'static [u8]) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind local proxy fixture");
    listener
        .set_nonblocking(true)
        .expect("set nonblocking listener");
    let address = listener.local_addr().unwrap();
    let response = response.to_vec();
    thread::spawn(move || {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let (mut stream, _) = loop {
            match listener.accept() {
                Ok(connection) => break connection,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(
                        std::time::Instant::now() < deadline,
                        "proxy request timed out"
                    );
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("proxy accept failed: {error}"),
            }
        };
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut request = [0; 4096];
        let size = stream.read(&mut request).expect("read proxy request");
        let request = String::from_utf8_lossy(&request[..size]);
        assert!(request.starts_with(expected_prefix), "{request}");
        stream.write_all(&response).expect("write proxy response");
    });
    format!("http://{address}")
}

#[test]
fn http_proxy_environment_routes_download_through_proxy() {
    if std::env::var(PROXY_WORKER).as_deref()
        == Ok("http_proxy_environment_routes_download_through_proxy")
    {
        let progress = std::sync::Mutex::new(Vec::new());
        let client = SystemUpdateHttpClient::default();
        let bytes = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(
                client.download_with_progress("http://fixture/asset", &|value| {
                    progress.lock().unwrap().push(value)
                }),
            )
            .unwrap();
        assert_eq!(bytes, b"fixture asset");
        assert_eq!(
            progress.lock().unwrap().last().unwrap().downloaded_bytes,
            13
        );
        return;
    }

    let proxy = serve_proxy_response(
        "GET http://fixture/asset HTTP/",
        b"HTTP/1.1 200 OK\r\nContent-Length: 13\r\nConnection: close\r\n\r\nfixture asset",
    );
    run_proxy_worker(
        "http_proxy_environment_routes_download_through_proxy",
        "HTTP_PROXY",
        &proxy,
    );
}

#[test]
fn lowercase_http_proxy_environment_routes_download_through_proxy() {
    if std::env::var(PROXY_WORKER).as_deref()
        == Ok("lowercase_http_proxy_environment_routes_download_through_proxy")
    {
        let bytes = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(SystemUpdateHttpClient::default().download("http://fixture/asset"))
            .unwrap();
        assert_eq!(bytes, b"fixture asset");
        return;
    }

    let proxy = serve_proxy_response(
        "GET http://fixture/asset HTTP/",
        b"HTTP/1.1 200 OK\r\nContent-Length: 13\r\nConnection: close\r\n\r\nfixture asset",
    );
    run_proxy_worker(
        "lowercase_http_proxy_environment_routes_download_through_proxy",
        "http_proxy",
        &proxy,
    );
}

#[test]
fn https_proxy_environment_routes_connect_through_proxy() {
    if std::env::var(PROXY_WORKER).as_deref()
        == Ok("https_proxy_environment_routes_connect_through_proxy")
    {
        let result = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(SystemUpdateHttpClient::default().download("https://fixture.invalid/asset"));
        let error = result.expect_err("proxy fixture rejects CONNECT before TLS");
        let chain = format!("{error:#}");
        assert!(chain.contains("failed to request update asset"), "{chain}");
        assert!(chain.contains("tunnel error"), "{chain}");
        return;
    }

    let proxy = serve_proxy_response(
        "CONNECT fixture.invalid:443 HTTP/",
        b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 29\r\nConnection: close\r\n\r\nproxy fixture rejected CONNECT",
    );
    run_proxy_worker(
        "https_proxy_environment_routes_connect_through_proxy",
        "HTTPS_PROXY",
        &proxy,
    );
}

#[test]
fn no_proxy_bypasses_environment_proxy() {
    if std::env::var(PROXY_WORKER).as_deref() == Ok("no_proxy_bypasses_environment_proxy") {
        let port = std::env::var("LAZYDB_NO_PROXY_TARGET_PORT").expect("target port");
        let result = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(
                SystemUpdateHttpClient::default()
                    .download(&format!("http://127.0.0.1:{port}/asset")),
            )
            .unwrap();
        assert_eq!(result, b"fixture asset");
        return;
    }

    let target = TcpListener::bind("127.0.0.1:0").expect("bind local target fixture");
    let port = target.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let (mut stream, _) = target.accept().expect("accept direct no_proxy request");
        let mut request = [0; 2048];
        let size = stream.read(&mut request).expect("read direct request");
        assert!(String::from_utf8_lossy(&request[..size]).starts_with("GET /asset HTTP/"));
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Length: 13\r\nConnection: close\r\n\r\nfixture asset",
            )
            .expect("respond to direct request");
    });
    let proxy = "http://127.0.0.1:1";
    let mut command = Command::new(std::env::current_exe().expect("integration test executable"));
    clear_proxy_environment(&mut command);
    let output = command
        .arg("--exact")
        .arg("no_proxy_bypasses_environment_proxy")
        .arg("--nocapture")
        .env(PROXY_WORKER, "no_proxy_bypasses_environment_proxy")
        .env("HTTP_PROXY", proxy)
        .env("http_proxy", proxy)
        .env("NO_PROXY", "127.0.0.1")
        .env("no_proxy", "127.0.0.1")
        .env("LAZYDB_NO_PROXY_TARGET_PORT", port.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("run isolated no_proxy worker");
    assert!(
        output.status.success(),
        "worker failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    server.join().expect("direct target server completed");
}

#[test]
#[ignore = "requires network access and a configured macOS system proxy"]
fn system_proxy_can_download_a_release_asset_without_proxy_environment() {
    if std::env::var(PROXY_WORKER).as_deref()
        == Ok("system_proxy_can_download_a_release_asset_without_proxy_environment")
    {
        let result = tokio::runtime::Runtime::new().unwrap().block_on(async {
            let progress = std::sync::Mutex::new(Vec::new());
            let client = SystemUpdateHttpClient::default();
            let bytes = client
                .download_with_progress(
                    "https://github.com/yelog/lazydb/releases/download/v0.1.7/lazydb_0.1.7_aarch64-apple-darwin.tar.xz",
                    &|value| progress.lock().unwrap().push(value),
                )
                .await?;
            assert!(!bytes.is_empty());
            let progress = progress.lock().unwrap();
            let last = progress.last().expect("download reports progress");
            assert_eq!(last.downloaded_bytes, bytes.len() as u64);
            if let Some(total_bytes) = last.total_bytes {
                assert_eq!(total_bytes, bytes.len() as u64);
            }
            Ok::<(), anyhow::Error>(())
        });
        result.unwrap();
        return;
    }

    let mut command = Command::new(std::env::current_exe().expect("integration test executable"));
    clear_proxy_environment(&mut command);
    let output = command
        .arg("--ignored")
        .arg("--exact")
        .arg("system_proxy_can_download_a_release_asset_without_proxy_environment")
        .arg("--nocapture")
        .env(
            PROXY_WORKER,
            "system_proxy_can_download_a_release_asset_without_proxy_environment",
        )
        .output()
        .expect("run isolated system proxy worker");
    assert!(
        output.status.success(),
        "worker failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
