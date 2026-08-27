use std::fmt;
use std::io::{BufRead, BufReader, Read};
use std::process::Command;
use std::process::Stdio;
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CommandOutput {
    pub(crate) stdout: String,
    pub(crate) stderr: String,
    pub(crate) exit_code: Option<i32>,
}

pub(crate) trait CommandRunner {
    fn run(&self, program: &str, args: &[&str]) -> Result<CommandOutput, CommandRunnerError>;

    fn run_with_cancellation(
        &self,
        program: &str,
        args: &[&str],
        is_cancelled: &dyn Fn() -> bool,
    ) -> Result<CommandOutput, CommandRunnerError> {
        if is_cancelled() {
            return Err(CommandRunnerError::Cancelled);
        }

        self.run(program, args)
    }

    fn run_with_cancellation_and_stdout_lines(
        &self,
        program: &str,
        args: &[&str],
        is_cancelled: &dyn Fn() -> bool,
        on_stdout_line: &mut dyn FnMut(&str),
    ) -> Result<CommandOutput, CommandRunnerError> {
        let output = self.run_with_cancellation(program, args, is_cancelled)?;
        for line in output.stdout.lines() {
            on_stdout_line(line);
        }
        Ok(output)
    }
}

#[derive(Debug)]
pub(crate) enum CommandRunnerError {
    Io(std::io::Error),
    Cancelled,
}

impl fmt::Display for CommandRunnerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "io error while running command: {error}"),
            Self::Cancelled => write!(f, "command execution cancelled"),
        }
    }
}

impl std::error::Error for CommandRunnerError {}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct SystemCommandRunner;

const CANCELLATION_POLL_INTERVAL: Duration = Duration::from_millis(20);
const STREAMED_STDOUT_CAPTURE_LIMIT: usize = 64 * 1024;
const STREAMED_STDOUT_LINE_CHANNEL_CAPACITY: usize = 128;

type PipeReaderHandle = JoinHandle<std::io::Result<Vec<u8>>>;

fn spawn_pipe_reader<R>(mut reader: R) -> PipeReaderHandle
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes)?;
        Ok(bytes)
    })
}

fn take_pipe_output(handle: &mut Option<PipeReaderHandle>) -> Result<Vec<u8>, std::io::Error> {
    let Some(handle) = handle.take() else {
        return Ok(Vec::new());
    };

    match handle.join() {
        Ok(result) => result,
        Err(_) => Err(std::io::Error::other("pipe reader thread panicked")),
    }
}

fn extend_bytes_up_to_limit(buffer: &mut Vec<u8>, chunk: &[u8], limit: usize) {
    let remaining = limit.saturating_sub(buffer.len());
    if remaining == 0 {
        return;
    }

    let bytes_to_copy = chunk.len().min(remaining);
    buffer.extend_from_slice(&chunk[..bytes_to_copy]);
}

fn emit_streamed_stdout_line(line: &str, on_stdout_line: &mut dyn FnMut(&str)) {
    on_stdout_line(line.trim_end_matches(['\r', '\n']));
}

fn drain_streamed_stdout_lines(
    line_rx: &mpsc::Receiver<String>,
    on_stdout_line: &mut dyn FnMut(&str),
) {
    while let Ok(line) = line_rx.try_recv() {
        emit_streamed_stdout_line(&line, on_stdout_line);
    }
}

fn drain_streamed_stdout_until_reader_finishes(
    line_rx: &mpsc::Receiver<String>,
    stdout_handle: &JoinHandle<std::io::Result<Vec<u8>>>,
    on_stdout_line: &mut dyn FnMut(&str),
) {
    loop {
        drain_streamed_stdout_lines(line_rx, on_stdout_line);
        if stdout_handle.is_finished() {
            break;
        }

        match line_rx.recv_timeout(CANCELLATION_POLL_INTERVAL) {
            Ok(line) => emit_streamed_stdout_line(&line, on_stdout_line),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
}

impl CommandRunner for SystemCommandRunner {
    fn run(&self, program: &str, args: &[&str]) -> Result<CommandOutput, CommandRunnerError> {
        let output = Command::new(program)
            .args(args)
            .output()
            .map_err(CommandRunnerError::Io)?;

        Ok(CommandOutput {
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            exit_code: output.status.code(),
        })
    }

    fn run_with_cancellation(
        &self,
        program: &str,
        args: &[&str],
        is_cancelled: &dyn Fn() -> bool,
    ) -> Result<CommandOutput, CommandRunnerError> {
        if is_cancelled() {
            return Err(CommandRunnerError::Cancelled);
        }

        let mut child = Command::new(program)
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(CommandRunnerError::Io)?;

        let mut stdout_handle = child.stdout.take().map(spawn_pipe_reader);
        let mut stderr_handle = child.stderr.take().map(spawn_pipe_reader);

        loop {
            if is_cancelled() {
                let _ = child.kill();
                let _ = child.wait();
                let _ = take_pipe_output(&mut stdout_handle);
                let _ = take_pipe_output(&mut stderr_handle);
                return Err(CommandRunnerError::Cancelled);
            }

            match child.try_wait() {
                Ok(Some(status)) => {
                    let stdout =
                        take_pipe_output(&mut stdout_handle).map_err(CommandRunnerError::Io)?;
                    let stderr =
                        take_pipe_output(&mut stderr_handle).map_err(CommandRunnerError::Io)?;

                    return Ok(CommandOutput {
                        stdout: String::from_utf8_lossy(&stdout).into_owned(),
                        stderr: String::from_utf8_lossy(&stderr).into_owned(),
                        exit_code: status.code(),
                    });
                }
                Ok(None) => thread::sleep(CANCELLATION_POLL_INTERVAL),
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = take_pipe_output(&mut stdout_handle);
                    let _ = take_pipe_output(&mut stderr_handle);
                    return Err(CommandRunnerError::Io(error));
                }
            }
        }
    }

    fn run_with_cancellation_and_stdout_lines(
        &self,
        program: &str,
        args: &[&str],
        is_cancelled: &dyn Fn() -> bool,
        on_stdout_line: &mut dyn FnMut(&str),
    ) -> Result<CommandOutput, CommandRunnerError> {
        if is_cancelled() {
            return Err(CommandRunnerError::Cancelled);
        }

        let mut child = Command::new(program)
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(CommandRunnerError::Io)?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| CommandRunnerError::Io(std::io::Error::other("missing child stdout")))?;
        let mut stderr_handle = child.stderr.take().map(spawn_pipe_reader);

        let (line_tx, line_rx) =
            mpsc::sync_channel::<String>(STREAMED_STDOUT_LINE_CHANNEL_CAPACITY);
        let stdout_handle = thread::spawn(move || -> std::io::Result<Vec<u8>> {
            let mut reader = BufReader::new(stdout);
            let mut line = String::new();
            let mut stdout_bytes = Vec::new();

            loop {
                line.clear();
                let bytes_read = reader.read_line(&mut line)?;
                if bytes_read == 0 {
                    break;
                }

                extend_bytes_up_to_limit(
                    &mut stdout_bytes,
                    line.as_bytes(),
                    STREAMED_STDOUT_CAPTURE_LIMIT,
                );
                let line_to_send = std::mem::take(&mut line);

                if line_tx.send(line_to_send).is_err() {
                    break;
                }
            }

            Ok(stdout_bytes)
        });

        let mut stdout_handle = Some(stdout_handle);

        loop {
            if is_cancelled() {
                let _ = child.kill();
                let _ = child.wait();
                let _ = take_pipe_output(&mut stderr_handle);
                drop(line_rx);
                if let Some(handle) = stdout_handle.take() {
                    let _ = handle.join();
                }
                return Err(CommandRunnerError::Cancelled);
            }

            drain_streamed_stdout_lines(&line_rx, on_stdout_line);

            match child.try_wait() {
                Ok(Some(status)) => {
                    if let Some(handle) = stdout_handle.as_ref() {
                        drain_streamed_stdout_until_reader_finishes(
                            &line_rx,
                            handle,
                            on_stdout_line,
                        );
                    }

                    let stdout = match stdout_handle.take() {
                        Some(handle) => match handle.join() {
                            Ok(result) => result,
                            Err(_) => Err(std::io::Error::other("stdout reader thread panicked")),
                        }
                        .map_err(CommandRunnerError::Io)?,
                        None => Vec::new(),
                    };
                    drain_streamed_stdout_lines(&line_rx, on_stdout_line);
                    let stderr =
                        take_pipe_output(&mut stderr_handle).map_err(CommandRunnerError::Io)?;

                    return Ok(CommandOutput {
                        stdout: String::from_utf8_lossy(&stdout).into_owned(),
                        stderr: String::from_utf8_lossy(&stderr).into_owned(),
                        exit_code: status.code(),
                    });
                }
                Ok(None) => thread::sleep(CANCELLATION_POLL_INTERVAL),
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = take_pipe_output(&mut stderr_handle);
                    drop(line_rx);
                    if let Some(handle) = stdout_handle.take() {
                        let _ = handle.join();
                    }
                    return Err(CommandRunnerError::Io(error));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    };
    use std::thread;
    use std::time::Duration;

    use super::{CommandRunner, CommandRunnerError, SystemCommandRunner};

    #[test]
    fn streamed_stdout_large_multiline_exit_completes_without_deadlock() {
        let script = "i=1; while [ $i -le 2048 ]; do printf 'line%04d\\n' \"$i\"; i=$((i+1)); done";
        let runner = SystemCommandRunner;
        let (result_tx, result_rx) = mpsc::channel();

        let handle = thread::spawn(move || {
            let mut streamed_lines = Vec::new();
            let result = runner.run_with_cancellation_and_stdout_lines(
                "sh",
                &["-c", script],
                &|| false,
                &mut |line| {
                    streamed_lines.push(line.to_owned());
                    thread::sleep(Duration::from_millis(1));
                },
            );

            let _ = result_tx.send((result, streamed_lines));
        });

        let (result, streamed_lines) = result_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("streamed run should finish without deadlock");

        handle.join().expect("streamed run thread should join");

        let output = result.expect("command should exit successfully");
        assert_eq!(output.exit_code, Some(0));
        assert_eq!(output.stdout.lines().count(), 2048);
        assert_eq!(streamed_lines.len(), 2048);
        assert_eq!(streamed_lines.first().map(String::as_str), Some("line0001"));
        assert_eq!(streamed_lines.last().map(String::as_str), Some("line2048"));
    }

    #[test]
    fn streamed_stdout_preserves_intermediate_lines_under_backpressure() {
        let script = "printf 'frame-1\\n'; printf '{\"event\":\\n'; printf 'frame-2\\n'; sleep 0.1";
        let runner = SystemCommandRunner;
        let (result_tx, result_rx) = mpsc::channel();

        let handle = thread::spawn(move || {
            let mut streamed_lines = Vec::new();
            let result = runner.run_with_cancellation_and_stdout_lines(
                "sh",
                &["-c", script],
                &|| false,
                &mut |line| {
                    streamed_lines.push(line.to_owned());
                    thread::sleep(Duration::from_millis(30));
                },
            );

            let _ = result_tx.send((result, streamed_lines));
        });

        let (result, streamed_lines) = result_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("streamed run should finish without deadlock");

        handle.join().expect("streamed run thread should join");

        let output = result.expect("command should exit successfully");
        assert_eq!(output.exit_code, Some(0));
        assert_eq!(
            streamed_lines,
            vec![
                "frame-1".to_owned(),
                "{\"event\":".to_owned(),
                "frame-2".to_owned(),
            ]
        );
    }

    #[test]
    fn streamed_stdout_cancellation_after_start_returns_cancelled_quickly() {
        let script = "printf 'start\\n'; while true; do printf 'tick\\n'; sleep 0.02; done";
        let runner = SystemCommandRunner;
        let cancelled = Arc::new(AtomicBool::new(false));
        let streamed_count = Arc::new(AtomicUsize::new(0));
        let (result_tx, result_rx) = mpsc::channel();

        let cancelled_for_thread = Arc::clone(&cancelled);
        let streamed_count_for_thread = Arc::clone(&streamed_count);

        let handle = thread::spawn(move || {
            let mut on_stdout_line = |_: &str| {
                if streamed_count_for_thread.fetch_add(1, Ordering::SeqCst) == 0 {
                    cancelled_for_thread.store(true, Ordering::SeqCst);
                }
            };

            let result = runner.run_with_cancellation_and_stdout_lines(
                "sh",
                &["-c", script],
                &|| cancelled.load(Ordering::SeqCst),
                &mut on_stdout_line,
            );

            let _ = result_tx.send(result);
        });

        let result = result_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("cancelled run should finish within timeout");

        handle.join().expect("cancelled run thread should join");

        assert!(matches!(result, Err(CommandRunnerError::Cancelled)));
        assert!(streamed_count.load(Ordering::SeqCst) > 0);
    }
}
