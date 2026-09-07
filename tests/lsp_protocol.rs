use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{Value, json};

struct LspProcess {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
}

impl LspProcess {
    fn start() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_lazydb"))
            .args(["lsp", "--stdio"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("start lazydb lsp server");
        Self {
            input: child.stdin.take().expect("LSP stdin"),
            output: BufReader::new(child.stdout.take().expect("LSP stdout")),
            child,
        }
    }

    fn request(&mut self, id: u64, method: &str, params: Value) -> Value {
        self.send(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }));
        loop {
            let response = self.read_message();
            if response.get("id") == Some(&json!(id)) {
                return response;
            }
        }
    }

    fn notify(&mut self, method: &str, params: Value) {
        self.send(json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }));
    }

    fn send(&mut self, message: Value) {
        let body = serde_json::to_vec(&message).expect("serialize LSP message");
        write!(self.input, "Content-Length: {}\r\n\r\n", body.len()).expect("write LSP header");
        self.input.write_all(&body).expect("write LSP body");
        self.input.flush().expect("flush LSP message");
    }

    fn read_message(&mut self) -> Value {
        let mut content_length = None;
        loop {
            let mut line = String::new();
            self.output.read_line(&mut line).expect("read LSP header");
            assert!(
                !line.is_empty(),
                "LSP server closed stdout before a response"
            );
            if line == "\r\n" {
                break;
            }
            let (name, value) = line.split_once(':').expect("valid LSP header");
            if name.eq_ignore_ascii_case("content-length") {
                content_length = Some(value.trim().parse::<usize>().expect("content length"));
            }
        }
        let length = content_length.expect("LSP content length");
        let mut body = vec![0; length];
        self.output.read_exact(&mut body).expect("read LSP body");
        serde_json::from_slice(&body).expect("decode LSP body")
    }

    fn notify_and_read(&mut self, method: &str, params: Value) -> Value {
        self.notify(method, params);
        self.read_message()
    }
}

impl Drop for LspProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn initialize_advertises_full_sync_and_shutdown_exits_cleanly() {
    let mut server = LspProcess::start();
    let response = server.request(
        1,
        "initialize",
        json!({
            "processId": null,
            "rootUri": null,
            "capabilities": {},
        }),
    );
    assert_eq!(response["id"], 1);
    assert_eq!(
        response["result"]["capabilities"]["textDocumentSync"], 1,
        "server must advertise full document synchronization"
    );

    server.notify("initialized", json!({}));
    let shutdown = server.request(2, "shutdown", Value::Null);
    assert_eq!(shutdown["id"], 2);
    assert!(shutdown.get("error").is_none());
    server.notify("exit", Value::Null);
    assert!(server.child.wait().expect("wait for LSP server").success());
}

#[test]
fn unsupported_request_returns_json_rpc_error_without_polluting_stdout() {
    let mut server = LspProcess::start();
    let _ = server.request(1, "initialize", json!({"capabilities": {}}));
    let response = server.request(2, "workspace/unknown", Value::Null);

    assert_eq!(response["id"], 2);
    assert!(response["error"]["code"].is_number());
    assert!(response.get("result").is_none());
}

#[test]
fn opening_invalid_sql_publishes_versioned_diagnostics() {
    let mut server = LspProcess::start();
    let _ = server.request(1, "initialize", json!({"capabilities": {}}));
    let uri = "file:///tmp/query.sql";
    let notification = server.notify_and_read(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": uri,
                "languageId": "sql",
                "version": 7,
                "text": "select * from users where"
            }
        }),
    );

    assert_eq!(notification["method"], "textDocument/publishDiagnostics");
    assert_eq!(notification["params"]["uri"], uri);
    assert_eq!(notification["params"]["version"], 7);
    assert_eq!(notification["params"]["diagnostics"][0]["source"], "lazydb");
}

#[test]
fn xml_sql_region_uses_sql_completion_and_reports_static_sql_errors() {
    let mut server = LspProcess::start();
    let _ = server.request(1, "initialize", json!({"capabilities": {}}));
    let uri = "file:///tmp/mapper.xml";
    let text = "<select id=\"x\">sel</select>";
    let notification = server.notify_and_read(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": uri,
                "languageId": "xml",
                "version": 1,
                "text": text
            }
        }),
    );
    assert_eq!(notification["method"], "textDocument/publishDiagnostics");
    assert_eq!(notification["params"]["diagnostics"][0]["source"], "lazydb");

    let response = server.request(
        2,
        "textDocument/completion",
        json!({
            "textDocument": {"uri": uri},
            "position": {"line": 0, "character": 18}
        }),
    );
    let items = &response["result"]["items"];
    assert!(
        items
            .as_array()
            .expect("completion list")
            .iter()
            .any(|item| item["label"] == "SELECT")
    );
}
