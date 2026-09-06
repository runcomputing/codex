use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use pretty_assertions::assert_eq;
use tokio::io::AsyncWriteExt;

use super::*;

#[test]
fn command_receives_the_active_thread_and_working_directory() {
    let cwd = std::env::temp_dir().join("codex-status-line-command");
    let context = StatusLineCommandContext::new(
        vec!["status-provider".to_string(), "--format=plain".to_string()],
        cwd.clone(),
        Some("thread-123".to_string()),
    )
    .expect("configured program should produce a command context");
    let command = status_line_command_process(&context)
        .expect("configured program should produce a process command");

    let envs = command
        .as_std()
        .get_envs()
        .map(|(key, value)| (key.to_os_string(), value.map(OsStr::to_os_string)))
        .collect::<BTreeMap<OsString, Option<OsString>>>();
    assert_eq!(
        command.as_std().get_program(),
        OsStr::new("status-provider")
    );
    assert_eq!(
        command.as_std().get_args().collect::<Vec<_>>(),
        vec![OsStr::new("--format=plain")]
    );
    assert_eq!(command.as_std().get_current_dir(), Some(cwd.as_path()));
    assert_eq!(
        envs.get(&OsString::from("CODEX_THREAD_ID")),
        Some(&Some(OsString::from("thread-123")))
    );
    assert_eq!(
        envs.get(&OsString::from("CODEX_STATUS_LINE_CWD")),
        Some(&Some(cwd.into_os_string()))
    );
}

#[test]
fn command_output_is_one_safe_trimmed_line() {
    assert_eq!(
        status_line_command_output(b"  custom:status\nignored second line\n"),
        Some("custom:status".to_string())
    );
    assert_eq!(
        status_line_command_output(b"\x1b[31mcustom:status\x1b[0m"),
        Some("custom:status".to_string())
    );
    assert_eq!(
        status_line_command_output(b"\x1b]0;untrusted title\x07custom:status"),
        Some("custom:status".to_string())
    );
    assert_eq!(status_line_command_output(b"\n"), None);

    let oversized = "a".repeat(MAX_STATUS_LINE_COMMAND_OUTPUT_CHARS + 1);
    assert_eq!(
        status_line_command_output(oversized.as_bytes()),
        Some("a".repeat(MAX_STATUS_LINE_COMMAND_OUTPUT_CHARS))
    );
}

#[tokio::test]
async fn command_output_cap_drains_the_oversized_stream() {
    let mut source = b"custom:status\n".to_vec();
    source.resize(MAX_STATUS_LINE_COMMAND_OUTPUT_BYTES * 128, b'x');
    let (mut writer, reader) = tokio::io::duplex(/*max_buf_size*/ 512);
    let write = tokio::spawn(async move { writer.write_all(&source).await });

    let output = read_capped_output(reader)
        .await
        .expect("output above the limit should be truncated");

    assert_eq!(output.len(), MAX_STATUS_LINE_COMMAND_OUTPUT_BYTES);
    assert_eq!(
        status_line_command_output(&output),
        Some("custom:status".to_string())
    );
    tokio::time::timeout(Duration::from_secs(1), write)
        .await
        .expect("reader should drain the writer after reaching the output cap")
        .expect("writer task should complete")
        .expect("writer should complete");
}

#[tokio::test]
async fn command_output_cap_drains_a_child_process() {
    let executable = std::env::current_exe().expect("current test executable");
    let listed_tests = Command::new(&executable)
        .arg("--list")
        .output()
        .expect("list test inventory");
    assert!(listed_tests.status.success());
    assert!(
        listed_tests.stdout.len() > MAX_STATUS_LINE_COMMAND_OUTPUT_BYTES * 128,
        "test inventory should exceed an OS pipe buffer"
    );
    let expected_status = status_line_command_output(&listed_tests.stdout)
        .expect("test inventory should begin with a status line");

    let context = StatusLineCommandContext::new(
        vec![
            executable.to_string_lossy().into_owned(),
            "--list".to_string(),
        ],
        std::env::current_dir().expect("current directory"),
        /*thread_id*/ None,
    )
    .expect("test executable should produce a command context");

    assert_eq!(
        run_status_line_command(context).await,
        Some(expected_status)
    );
}

#[test]
fn command_without_active_thread_clears_thread_environment() {
    let context = StatusLineCommandContext::new(
        vec!["status-provider".to_string()],
        PathBuf::from("workspace"),
        None,
    )
    .expect("configured program should produce a command context");
    let command = status_line_command_process(&context)
        .expect("configured program should produce a process command");
    let envs = command
        .as_std()
        .get_envs()
        .map(|(key, value)| (key.to_os_string(), value.map(OsStr::to_os_string)))
        .collect::<BTreeMap<OsString, Option<OsString>>>();

    assert_eq!(envs.get(&OsString::from("CODEX_THREAD_ID")), Some(&None));
}

#[test]
fn blank_program_does_not_create_a_command_context() {
    assert_eq!(
        StatusLineCommandContext::new(vec!["  ".to_string()], PathBuf::from("workspace"), None,),
        None
    );
}
