//! Bounded execution of the external OCR engine.
//!
//! The engine is an untrusted, heavyweight, third-party program (typically a wrapper
//! script that starts a Python interpreter running PaddleOCR). This module makes sure
//! that, whatever it does, one OCR request cannot:
//!
//! * leave descendants running after a timeout or a dropped request (the whole process
//!   tree is killed, not just the direct child),
//! * buffer unbounded output in memory (stdout is capped, stderr keeps only a short head),
//! * run forever (a single wall-clock budget covers start-up, runtime and output).
//!
//! Tree termination uses the `process-wrap` crate, which keeps the platform-specific
//! (and partly `unsafe`) code out of this repository:
//!
//! * **Unix**: the engine becomes the leader of a new process group and the group is
//!   sent `SIGKILL`. Descendants that stay in the group are killed with it. A descendant
//!   that deliberately creates its own session or group escapes this.
//! * **Windows**: the engine is assigned to a Job Object configured to kill every
//!   process in the job when it is closed or terminated.
//!
//! `wait()` on either wrapper completes only when every process of the group/job has
//! exited, so a descendant that outlives the engine does not slip past unnoticed: the run
//! simply keeps going until the time budget ends and the tree is killed. Kills happen
//! while the group/job still exists, so a recycled process id is never signalled.
//!
//! Not covered: a descendant that deliberately leaves the group (Unix `setsid`/`setpgid`)
//! or breaks away from the job (Windows) is outside what these mechanisms can reach.

use std::path::Path;
use std::process::{ExitStatus, Stdio};
use std::time::Duration;

use process_wrap::tokio::*;
use tokio::io::{AsyncRead, AsyncReadExt};

/// Largest stdout accepted from the engine. A dense invoice produces well under
/// 1 MiB of token JSON; anything beyond this is a malfunction, not a bill.
pub const MAX_STDOUT_BYTES: usize = 4 * 1024 * 1024;

/// How much of the engine's stderr is kept for the server log. The rest is read and
/// discarded so the engine never blocks on a full pipe.
pub const STDERR_CAPTURE_BYTES: usize = 16 * 1024;

/// How long to wait for a killed process tree to be reaped before giving up on it.
const REAP_GRACE: Duration = Duration::from_secs(5);

#[derive(Debug)]
pub enum RunError {
    /// The executable could not be started.
    Spawn(std::io::Error),
    /// Waiting for, or reading from, the engine failed.
    Io(std::io::Error),
    /// The wall-clock budget elapsed; the process tree was killed.
    TimedOut,
    /// stdout exceeded [`MAX_STDOUT_BYTES`]; the process tree was killed.
    OutputTooLarge,
}

#[derive(Debug)]
pub struct EngineOutput {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    /// At most [`STDERR_CAPTURE_BYTES`] of the engine's stderr.
    pub stderr: Vec<u8>,
}

/// Owns the spawned child and kills its whole tree if dropped while still armed, which
/// covers a cancelled request (the handler future being dropped) and panics.
struct TreeGuard {
    child: Option<Box<dyn TokioChildWrapper>>,
}

impl TreeGuard {
    fn child_mut(&mut self) -> &mut dyn TokioChildWrapper {
        self.child
            .as_deref_mut()
            .expect("child is present until the guard is disarmed")
    }

    /// The engine exited by itself and was reaped: nothing is left to kill, and signalling
    /// its (now free) process id could hit an unrelated process.
    fn disarm(&mut self) {
        self.child = None;
    }

    /// Kills the whole tree, then reaps the leader so no zombie is left behind.
    async fn kill_and_reap(&mut self) {
        let Some(mut child) = self.child.take() else {
            return;
        };
        if let Err(e) = child.start_kill() {
            tracing::warn!(error = %e, "failed to signal the OCR process tree");
        }
        // Remaining members die with the group/job; this just collects the leader.
        let _ = tokio::time::timeout(REAP_GRACE, Box::into_pin(child.wait())).await;
    }
}

impl Drop for TreeGuard {
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            // Synchronous signal only; tokio reaps the orphaned leader in the background.
            let _ = child.start_kill();
        }
    }
}

/// Reads until EOF, failing as soon as more than `cap` bytes arrive.
async fn read_capped<R: AsyncRead + Unpin>(mut reader: R, cap: usize) -> Result<Vec<u8>, RunError> {
    let mut out = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let n = reader.read(&mut chunk).await.map_err(RunError::Io)?;
        if n == 0 {
            return Ok(out);
        }
        if out.len() + n > cap {
            return Err(RunError::OutputTooLarge);
        }
        out.extend_from_slice(&chunk[..n]);
    }
}

/// Reads until EOF keeping only the first `keep` bytes; the rest is discarded.
async fn read_head<R: AsyncRead + Unpin>(mut reader: R, keep: usize) -> Result<Vec<u8>, RunError> {
    let mut out = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        // A broken stderr pipe is not worth failing the run for.
        let n = reader.read(&mut chunk).await.unwrap_or(0);
        if n == 0 {
            return Ok(out);
        }
        let room = keep.saturating_sub(out.len());
        out.extend_from_slice(&chunk[..n.min(room)]);
    }
}

/// Runs `<cmd> <image_path>` with the safety limits described in the module docs.
pub async fn run_engine(
    cmd: &str,
    image_path: &Path,
    timeout: Duration,
) -> Result<EngineOutput, RunError> {
    let mut wrap = TokioCommandWrap::with_new(cmd, |command| {
        command
            .arg(image_path)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
    });
    #[cfg(unix)]
    wrap.wrap(ProcessGroup::leader());
    #[cfg(windows)]
    wrap.wrap(JobObject);
    wrap.wrap(KillOnDrop);

    let mut child = wrap.spawn().map_err(RunError::Spawn)?;
    let stdout = child
        .stdout()
        .take()
        .expect("stdout was configured as piped");
    let stderr = child
        .stderr()
        .take()
        .expect("stderr was configured as piped");
    let mut guard = TreeGuard { child: Some(child) };

    let outcome = tokio::time::timeout(timeout, async {
        let wait = async {
            Box::into_pin(guard.child_mut().wait())
                .await
                .map_err(RunError::Io)
        };
        tokio::try_join!(
            wait,
            read_capped(stdout, MAX_STDOUT_BYTES),
            read_head(stderr, STDERR_CAPTURE_BYTES)
        )
    })
    .await;

    match outcome {
        Ok(Ok((status, stdout, stderr))) => {
            guard.disarm();
            Ok(EngineOutput {
                status,
                stdout,
                stderr,
            })
        }
        Ok(Err(e)) => {
            guard.kill_and_reap().await;
            Err(e)
        }
        Err(_elapsed) => {
            guard.kill_and_reap().await;
            Err(RunError::TimedOut)
        }
    }
}

#[cfg(test)]
mod tests;
