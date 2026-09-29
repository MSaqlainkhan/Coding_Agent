use saqlain_code::agent::{Agent, StepOutcome};
use saqlain_code::config::Config;
use saqlain_code::types::PermissionMode;
use saqlain_code::workspace::Workspace;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;

/// A tiny single-purpose HTTP server that answers every POST with the next
/// scripted JSON response body, in order. This lets us test the real agent
/// loop (HTTP call -> tool dispatch -> real filesystem/process actions ->
/// next HTTP call) without needing a live LLM.
struct MockServer {
    port: u16,
}

fn spawn_mock_server(responses: Vec<String>) -> MockServer {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock server");
    let port = listener.local_addr().unwrap().port();
    let counter = Arc::new(AtomicUsize::new(0));
    let responses = Arc::new(responses);

    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            let counter = counter.clone();
            let responses = responses.clone();
            thread::spawn(move || {
                handle_connection(stream, &counter, &responses);
            });
        }
    });

    // Give the listener a moment to be ready.
    thread::sleep(std::time::Duration::from_millis(50));
    MockServer { port }
}

fn handle_connection(mut stream: TcpStream, counter: &AtomicUsize, responses: &[String]) {
    let mut buf = [0u8; 8192];
    let mut received = Vec::new();

    // Read until we see the end of headers, then read the declared body length.
    loop {
        let n = match stream.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        received.extend_from_slice(&buf[..n]);
        if received.windows(4).any(|w| w == b"\r\n\r\n") {
            let header_text = String::from_utf8_lossy(&received);
            if let Some(len) = content_length(&header_text) {
                let body_start = header_text.find("\r\n\r\n").unwrap() + 4;
                if received.len() - body_start >= len {
                    break;
                }
            } else {
                break;
            }
        }
    }

    let idx = counter.fetch_add(1, Ordering::SeqCst);
    let body = responses
        .get(idx)
        .cloned()
        .unwrap_or_else(|| responses.last().cloned().unwrap_or_default());

    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

fn content_length(headers: &str) -> Option<usize> {
    for line in headers.lines() {
        if let Some(rest) = line.to_lowercase().strip_prefix("content-length:") {
            return rest.trim().parse().ok();
        }
    }
    None
}

fn tool_call_response(id: &str, name: &str, args_json: &str) -> String {
    format!(
        r#"{{"choices":[{{"message":{{"role":"assistant","content":null,"tool_calls":[{{"id":"{id}","type":"function","function":{{"name":"{name}","arguments":"{args}"}}}}]}},"finish_reason":"tool_calls"}}]}}"#,
        id = id,
        name = name,
        args = args_json.replace('\\', "\\\\").replace('"', "\\\"")
    )
}

fn final_text_response(text: &str) -> String {
    format!(
        r#"{{"choices":[{{"message":{{"role":"assistant","content":"{text}"}},"finish_reason":"stop"}}]}}"#,
        text = text
    )
}

fn test_config(base_url: String) -> Config {
    Config {
        base_url,
        model: "gemini-3.5-flash".to_string(),
        api_key: "anything".to_string(),
        permission_mode: PermissionMode::Auto,
        max_iterations: 10,
        command_timeout_secs: 15,
        verbose: false,
        auto: true,
    }
}

#[tokio::test]
async fn agent_loop_executes_real_write_file_tool_call() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(dir.path().to_path_buf()).unwrap();

    let responses = vec![
        tool_call_response(
            "call_1",
            "write_file",
            r#"{"path":"hello.txt","content":"Hello from the agent loop test!"}"#,
        ),
        final_text_response("Created hello.txt."),
    ];
    let server = spawn_mock_server(responses);
    let base_url = format!("http://127.0.0.1:{}/v1", server.port);

    let config = test_config(base_url);
    let mut agent = Agent::new(&config, workspace);

    let outcome = agent
        .handle_user_message("please create hello.txt")
        .await
        .expect("agent loop should succeed");

    match outcome {
        StepOutcome::Final(text) => assert_eq!(text, "Created hello.txt."),
        _ => panic!("expected a final text outcome"),
    }

    // The critical assertion: the Rust program actually created the file on
    // disk as a result of the tool call, independent of what the model said.
    let written = std::fs::read_to_string(dir.path().join("hello.txt")).unwrap();
    assert_eq!(written, "Hello from the agent loop test!");
}

#[tokio::test]
async fn agent_loop_runs_multiple_tool_calls_before_final_answer() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(dir.path().to_path_buf()).unwrap();

    let responses = vec![
        tool_call_response(
            "call_1",
            "write_file",
            r#"{"path":"a.txt","content":"first"}"#,
        ),
        tool_call_response(
            "call_2",
            "read_file",
            r#"{"path":"a.txt"}"#,
        ),
        final_text_response("Done: created and verified a.txt."),
    ];
    let server = spawn_mock_server(responses);
    let base_url = format!("http://127.0.0.1:{}/v1", server.port);

    let config = test_config(base_url);
    let mut agent = Agent::new(&config, workspace);

    let outcome = agent
        .handle_user_message("create and verify a.txt")
        .await
        .expect("agent loop should succeed");

    assert!(matches!(outcome, StepOutcome::Final(_)));
    assert!(dir.path().join("a.txt").exists());

    // Conversation history should contain the assistant tool_calls messages
    // and their matching tool results, preserving tool_call_id linkage.
    let messages = agent.history().messages();
    let tool_result_ids: Vec<&str> = messages
        .iter()
        .filter(|m| m.role == "tool")
        .filter_map(|m| m.tool_call_id.as_deref())
        .collect();
    assert!(tool_result_ids.contains(&"call_1"));
    assert!(tool_result_ids.contains(&"call_2"));
}

#[tokio::test]
async fn agent_loop_respects_max_iterations() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(dir.path().to_path_buf()).unwrap();

    // The server always returns another tool call, so the agent should stop
    // itself at max_iterations rather than looping forever.
    let responses: Vec<String> = (0..3)
        .map(|i| tool_call_response(&format!("call_{i}"), "list_files", r#"{"path":"."}"#))
        .collect();
    let server = spawn_mock_server(responses);
    let base_url = format!("http://127.0.0.1:{}/v1", server.port);

    let mut config = test_config(base_url);
    config.max_iterations = 3;
    let mut agent = Agent::new(&config, workspace);

    let outcome = agent
        .handle_user_message("loop forever please")
        .await
        .expect("agent loop should not error");

    assert!(matches!(outcome, StepOutcome::MaxIterations));
}
