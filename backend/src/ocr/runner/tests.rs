//! Regression tests for the OCR process runner. They use small platform scripts as fake
//! engines. "Is a descendant still alive?" is answered with a heartbeat file: the
//! descendant appends to it continuously, so once the tree is really dead the file stops
//! growing. That works identically on Unix and Windows without handling process ids.

use super::*;
use std::path::PathBuf;
use std::time::Instant;

fn temp_path(tag: &str, ext: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("artha_run_{tag}_{}_{n}.{ext}", std::process::id()))
}

/// Writes an executable script for the current platform and returns its path.
fn write_script(tag: &str, windows_body: &str, unix_body: &str) -> PathBuf {
    #[cfg(windows)]
    {
        let _ = unix_body;
        let p = temp_path(tag, "cmd");
        std::fs::write(&p, windows_body).unwrap();
        p
    }
    #[cfg(not(windows))]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = windows_body;
        let p = temp_path(tag, "sh");
        std::fs::write(&p, unix_body).unwrap();
        let mut perms = std::fs::metadata(&p).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&p, perms).unwrap();
        p
    }
}

fn image() -> PathBuf {
    let p = temp_path("img", "png");
    std::fs::write(&p, b"x").unwrap();
    p
}

fn file_len(p: &Path) -> u64 {
    std::fs::metadata(p).map(|m| m.len()).unwrap_or(0)
}

/// Script that starts a background descendant appending to `heartbeat` (about every
/// 100ms on Unix, every second on Windows) and then hangs itself.
fn descendant_script(tag: &str, heartbeat: &Path) -> PathBuf {
    let hb = heartbeat.display();
    write_script(
        tag,
        &format!(
            "@echo off\r\nstart /b ping -n 60 127.0.0.1 >> \"{hb}\"\r\nping -n 60 127.0.0.1 >nul\r\n"
        ),
        &format!("#!/bin/sh\n( while true; do echo x >> '{hb}'; sleep 0.1; done ) &\nsleep 30\n"),
    )
}

/// Asserts the heartbeat file stops growing, i.e. nothing is still writing to it.
async fn assert_heartbeat_stopped(heartbeat: &Path) {
    tokio::time::sleep(Duration::from_millis(1500)).await; // let killed processes die
    let settled = file_len(heartbeat);
    tokio::time::sleep(Duration::from_millis(2500)).await;
    assert_eq!(
        file_len(heartbeat),
        settled,
        "a descendant of the OCR engine is still running"
    );
}

async fn wait_for_heartbeat(heartbeat: &Path) {
    for _ in 0..100 {
        if file_len(heartbeat) > 0 {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("the descendant never started writing its heartbeat");
}

#[tokio::test]
async fn successful_run_returns_stdout_and_exit_status() {
    let script = write_script("ok", "@echo off\r\necho [1]\r\n", "#!/bin/sh\necho '[1]'\n");
    let out = run_engine(&script.to_string_lossy(), &image(), Duration::from_secs(20))
        .await
        .expect("engine should run");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "[1]");
    let _ = std::fs::remove_file(script);
}

#[tokio::test]
async fn non_zero_exit_is_reported_with_captured_stderr() {
    let script = write_script(
        "fail",
        "@echo off\r\necho boom 1>&2\r\nexit /b 3\r\n",
        "#!/bin/sh\necho boom >&2\nexit 3\n",
    );
    let out = run_engine(&script.to_string_lossy(), &image(), Duration::from_secs(20))
        .await
        .expect("a failing engine still returns its output");
    assert_eq!(out.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&out.stderr).contains("boom"));
    let _ = std::fs::remove_file(script);
}

#[tokio::test]
async fn missing_executable_is_a_spawn_error() {
    let r = run_engine(
        "/definitely/missing/dir/ocr-engine",
        &image(),
        Duration::from_secs(5),
    )
    .await;
    assert!(matches!(r, Err(RunError::Spawn(_))), "{r:?}");
}

#[tokio::test]
async fn timeout_returns_promptly_and_kills_the_engine() {
    let script = write_script(
        "hang",
        "@echo off\r\nping -n 60 127.0.0.1 >nul\r\n",
        "#!/bin/sh\nsleep 30\n",
    );
    let started = Instant::now();
    let r = run_engine(
        &script.to_string_lossy(),
        &image(),
        Duration::from_millis(300),
    )
    .await;
    assert!(matches!(r, Err(RunError::TimedOut)), "{r:?}");
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "timeout must not wait for the engine to finish"
    );
    let _ = std::fs::remove_file(script);
}

#[tokio::test]
async fn timeout_kills_descendants_of_the_engine() {
    let hb = temp_path("hb_timeout", "txt");
    let script = descendant_script("desc_timeout", &hb);
    let r = run_engine(
        &script.to_string_lossy(),
        &image(),
        Duration::from_millis(2000),
    )
    .await;
    assert!(matches!(r, Err(RunError::TimedOut)), "{r:?}");
    assert!(
        file_len(&hb) > 0,
        "the descendant must have been running before the timeout"
    );
    assert_heartbeat_stopped(&hb).await;
    let _ = std::fs::remove_file(script);
    let _ = std::fs::remove_file(hb);
}

#[tokio::test]
async fn dropping_the_request_kills_the_engine_and_its_descendants() {
    let hb = temp_path("hb_cancel", "txt");
    let script = descendant_script("desc_cancel", &hb);
    let cmd = script.to_string_lossy().into_owned();
    let img = image();
    let task = tokio::spawn(async move {
        run_engine(&cmd, &img, Duration::from_secs(120))
            .await
            .is_ok()
    });
    wait_for_heartbeat(&hb).await;
    // A client that disconnects makes the server drop the handler future.
    task.abort();
    let _ = task.await;
    assert_heartbeat_stopped(&hb).await;
    let _ = std::fs::remove_file(script);
    let _ = std::fs::remove_file(hb);
}

#[tokio::test]
async fn excessive_stdout_is_rejected_and_the_engine_is_killed() {
    let script = write_script(
        "flood",
        // ~7.6 MB of output, then hang so only the size check can end the run.
        "@echo off\r\nfor /l %%i in (1,1,200000) do @echo xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx\r\nping -n 60 127.0.0.1 >nul\r\n",
        "#!/bin/sh\nyes | head -c 8000000\nsleep 30\n",
    );
    let started = Instant::now();
    let r = run_engine(&script.to_string_lossy(), &image(), Duration::from_secs(60)).await;
    assert!(matches!(r, Err(RunError::OutputTooLarge)), "{r:?}");
    assert!(
        started.elapsed() < Duration::from_secs(45),
        "oversize output must end the run, not wait for the timeout"
    );
    let _ = std::fs::remove_file(script);
}

#[tokio::test]
async fn huge_stderr_is_truncated_not_accumulated() {
    let script = write_script(
        "noisy",
        "@echo off\r\nfor /l %%i in (1,1,20000) do @echo yyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyy 1>&2\r\necho []\r\n",
        "#!/bin/sh\nyes y | head -c 3000000 >&2\necho '[]'\n",
    );
    let out = run_engine(&script.to_string_lossy(), &image(), Duration::from_secs(60))
        .await
        .expect("noisy stderr must not fail the run");
    assert!(out.status.success());
    assert!(
        out.stderr.len() <= STDERR_CAPTURE_BYTES,
        "{}",
        out.stderr.len()
    );
    assert!(!out.stderr.is_empty());
    let _ = std::fs::remove_file(script);
}
