use std::process::Command;

/// These are black-box integration tests that exercise the compiled binary's
/// underlying library logic indirectly is not possible without a lib target,
/// so we instead verify the workspace behaves correctly via cargo itself.
/// Detailed unit tests for tool execution live in `src/tools.rs`.
#[test]
fn binary_builds_and_reports_help() {
    let output = Command::new(env!("CARGO_BIN_EXE_saqlain-code"))
        .arg("--help")
        .output()
        .expect("failed to run binary");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("saqlain-code"));
}

#[test]
fn binary_reports_version() {
    let output = Command::new(env!("CARGO_BIN_EXE_saqlain-code"))
        .arg("--version")
        .output()
        .expect("failed to run binary");
    assert!(output.status.success());
}
