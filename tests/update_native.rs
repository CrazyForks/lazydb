#![cfg(unix)]

use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    os::unix::fs::symlink,
    process::Command,
    thread,
};

#[test]
fn installed_launcher_reports_native_manager() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("config");
    let version = env!("CARGO_PKG_VERSION");
    let release = data.join("releases").join(version);
    fs::create_dir_all(&release).unwrap();
    fs::copy(env!("CARGO_BIN_EXE_lazydb"), release.join("lazydb")).unwrap();
    symlink(&release, data.join("current")).unwrap();
    let launcher = dir.path().join("bin/lazydb");
    fs::create_dir_all(launcher.parent().unwrap()).unwrap();
    symlink("../config/current/lazydb", &launcher).unwrap();
    fs::write(
        data.join("install.json"),
        serde_json::json!({
            "schema": 1, "product": "lazydb", "manager": "native",
            "channel": "stable", "version": version,
            "target": "x86_64-unknown-linux-gnu", "path": launcher
        })
        .to_string(),
    )
    .unwrap();

    let targets = [
        "x86_64-apple-darwin",
        "aarch64-apple-darwin",
        "x86_64-unknown-linux-gnu",
        "aarch64-unknown-linux-gnu",
        "x86_64-pc-windows-msvc",
    ];
    let assets: serde_json::Map<String, serde_json::Value> = targets.into_iter().map(|target| {
        let extension = if target.ends_with("msvc") { "zip" } else { "tar.xz" };
        (target.to_owned(), serde_json::json!({
            "url": format!("https://github.com/yelog/lazydb/releases/download/v9.9.9/lazydb_9.9.9_{target}.{extension}"),
            "sha256": "a".repeat(64)
        }))
    }).collect();
    let body = serde_json::json!({
        "schema": 1, "product": "lazydb", "channel": "stable", "version": "9.9.9",
        "tag": "v9.9.9", "prerelease": false, "published_at": "2026-09-09T00:00:00Z",
        "release_url": "https://github.com/yelog/lazydb/releases/tag/v9.9.9", "assets": assets
    })
    .to_string();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let server = thread::spawn(move || {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(
                        std::time::Instant::now() < deadline,
                        "manifest request timed out"
                    );
                    thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(error) => panic!("{error}"),
            }
        };
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let mut request = [0; 4096];
        let size = stream.read(&mut request).unwrap();
        assert!(String::from_utf8_lossy(&request[..size]).starts_with("GET /stable.json "));
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
    });
    let output = Command::new(&launcher)
        .args(["update", "--check", "--json"])
        .env("HOME", dir.path())
        .env("XDG_CONFIG_HOME", dir.path().join("xdg-config"))
        .env("XDG_DATA_HOME", dir.path().join("xdg-data"))
        .env("LAZYDB_CONFIG_HOME", &data)
        .env("LAZYDB_CHANNEL_BASE_URL", format!("http://{address}"))
        .env("NO_PROXY", "127.0.0.1")
        .env("no_proxy", "127.0.0.1")
        .output()
        .unwrap();
    server.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["manager"], "native", "{report}");
    assert_eq!(report["status"], "update_available", "{report}");
    assert_eq!(report["target_version"], "9.9.9");
}
