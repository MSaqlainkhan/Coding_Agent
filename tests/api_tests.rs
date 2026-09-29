use saqlain_code::types::{ChatMessage, FunctionCall, ToolCall};

#[test]
fn chat_message_user_serializes_without_tool_fields() {
    let msg = ChatMessage::user("hello");
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains("\"role\":\"user\""));
    assert!(json.contains("\"content\":\"hello\""));
    assert!(!json.contains("tool_calls"));
}

#[test]
fn chat_message_assistant_tool_calls_serializes_correctly() {
    let call = ToolCall {
        id: "call_1".to_string(),
        kind: "function".to_string(),
        function: FunctionCall {
            name: "write_file".to_string(),
            arguments: "{\"path\":\"a.txt\",\"content\":\"hi\"}".to_string(),
        },
    };
    let msg = ChatMessage::assistant_tool_calls(vec![call]);
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains("\"role\":\"assistant\""));
    assert!(json.contains("write_file"));
    assert!(!json.contains("\"content\""));
}

#[test]
fn tool_result_message_preserves_tool_call_id_and_name() {
    let msg = ChatMessage::tool_result("call_42", "read_file", "file contents here");
    assert_eq!(msg.role, "tool");
    assert_eq!(msg.tool_call_id.as_deref(), Some("call_42"));
    assert_eq!(msg.name.as_deref(), Some("read_file"));
    assert_eq!(msg.content.as_deref(), Some("file contents here"));
}

#[test]
fn parses_example_tool_call_response_from_spec() {
    // This mirrors the exact example response format from the project spec.
    let raw = r#"
    {
      "choices": [
        {
          "message": {
            "role": "assistant",
            "content": null,
            "tool_calls": [
              {
                "id": "call_bd0e5fa3",
                "type": "function",
                "function": {
                  "name": "write_file",
                  "arguments": "{\"content\":\"HELLO FROM CLINE\",\"path\":\"test.txt\"}"
                }
              }
            ]
          },
          "finish_reason": "tool_calls"
        }
      ]
    }
    "#;

    let parsed: saqlain_code::types::ChatCompletionResponse =
        serde_json::from_str(raw).expect("should parse spec example response");

    let choice = &parsed.choices[0];
    assert_eq!(choice.finish_reason.as_deref(), Some("tool_calls"));

    let tool_calls = choice.message.tool_calls.as_ref().expect("should have tool_calls");
    assert_eq!(tool_calls.len(), 1);
    assert_eq!(tool_calls[0].function.name, "write_file");

    let args: serde_json::Value = serde_json::from_str(&tool_calls[0].function.arguments).unwrap();
    assert_eq!(args["path"], "test.txt");
    assert_eq!(args["content"], "HELLO FROM CLINE");
}

#[test]
fn tool_definitions_include_all_required_tools() {
    let defs = saqlain_code::tools::tool_definitions();
    let names: Vec<&str> = defs.iter().map(|d| d.function.name.as_str()).collect();
    for expected in [
        "list_files",
        "read_file",
        "write_file",
        "edit_file",
        "delete_file",
        "search_code",
        "run_command",
    ] {
        assert!(names.contains(&expected), "missing tool definition: {expected}");
    }
}
