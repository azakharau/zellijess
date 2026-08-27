mod command_runner;
mod models;
mod parsing;

use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};

use command_runner::{CommandOutput, CommandRunnerError};
pub(crate) use command_runner::{CommandRunner, SystemCommandRunner};
pub(crate) use models::{PaneInfo, SessionInfo, SessionState, TabInfo};
use parsing::{ParseError, parse_panes_output, parse_sessions_output, parse_tabs_output};
use serde::Deserialize;

#[cfg(test)]
pub(crate) use parsing::{
    parse_panes_output as parse_panes_output_for_tests,
    parse_sessions_output as parse_sessions_output_for_tests,
    parse_tabs_output as parse_tabs_output_for_tests,
};

const ZELLIJ_PROGRAM: &str = "zellij";
const LIST_SESSIONS_ARGS: [&str; 2] = ["list-sessions", "--no-formatting"];
const LIST_TABS_ARGS: [&str; 4] = ["action", "list-tabs", "--json", "--all"];
const LIST_PANES_ARGS: [&str; 4] = ["action", "list-panes", "--json", "--all"];

const COMMAND_CONTRACT: [&str; 7] = [
    "zellij list-sessions --no-formatting",
    "zellij --session <session> action list-tabs --json --all",
    "zellij --session <session> action list-panes --json --all",
    "zellij --session <session> action dump-screen --pane-id <id> --ansi",
    "zellij --session <session> subscribe --pane-id <id> --ansi --format json --scrollback 0",
    "zellij action list-tabs --json --all (best-effort current-context)",
    "zellij action list-panes --json --all (best-effort current-context)",
];

pub(crate) fn command_contract() -> &'static [&'static str] {
    &COMMAND_CONTRACT
}

#[derive(Debug, Clone)]
pub(crate) struct ParsedRuntimeSnapshot {
    pub(crate) sessions: Vec<SessionInfo>,
    pub(crate) tabs: Vec<TabInfo>,
    pub(crate) panes: Vec<PaneInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PaneSnapshot {
    Ready(String),
    Empty,
}

pub(crate) fn parse_runtime_snapshot(
    sessions_raw: &str,
    tabs_raw: &str,
    panes_raw: &str,
) -> Result<ParsedRuntimeSnapshot, RuntimeDiscoveryError> {
    let sessions = parse_sessions_output(sessions_raw).map_err(RuntimeDiscoveryError::Parse)?;
    let tabs = parse_tabs_output(tabs_raw).map_err(RuntimeDiscoveryError::Parse)?;
    let panes = parse_panes_output(panes_raw).map_err(RuntimeDiscoveryError::Parse)?;

    Ok(ParsedRuntimeSnapshot {
        sessions,
        tabs,
        panes,
    })
}

#[derive(Debug)]
pub(crate) enum RuntimeDiscoveryError {
    CommandRunner(CommandRunnerError),
    Cancelled {
        command: String,
    },
    CommandFailed {
        command: String,
        exit_code: Option<i32>,
        stderr: String,
    },
    RuntimeDiagnostic {
        command: String,
        stdout: String,
        stderr: String,
    },
    Parse(ParseError),
}

impl fmt::Display for RuntimeDiscoveryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CommandRunner(error) => write!(f, "command runner failed: {error}"),
            Self::Cancelled { command } => write!(f, "`{command}` was cancelled"),
            Self::CommandFailed {
                command,
                exit_code,
                stderr,
            } => {
                let stderr = stderr.trim();
                if stderr.is_empty() {
                    write!(f, "`{command}` failed with exit code {:?}", exit_code)
                } else {
                    write!(
                        f,
                        "`{command}` failed with exit code {:?}: {stderr}",
                        exit_code
                    )
                }
            }
            Self::RuntimeDiagnostic {
                command,
                stdout,
                stderr,
            } => {
                let stderr = stderr.trim();
                let stdout = stdout.trim();
                let diagnostic = if stderr.is_empty() { stdout } else { stderr };
                if diagnostic.is_empty() {
                    write!(
                        f,
                        "`{command}` returned a runtime diagnostic with exit code 0; session targeting may be required"
                    )
                } else {
                    write!(
                        f,
                        "`{command}` returned a runtime diagnostic with exit code 0: {diagnostic}"
                    )
                }
            }
            Self::Parse(error) => write!(f, "failed to parse zellij output: {error}"),
        }
    }
}

impl std::error::Error for RuntimeDiscoveryError {}

pub(crate) struct RuntimeDiscovery<R: CommandRunner> {
    runner: R,
}

impl<R: CommandRunner> RuntimeDiscovery<R> {
    pub(crate) fn new(runner: R) -> Self {
        Self { runner }
    }

    pub(crate) fn list_sessions(&self) -> Result<Vec<SessionInfo>, RuntimeDiscoveryError> {
        let output = self.run_checked(
            "zellij list-sessions --no-formatting".to_owned(),
            &LIST_SESSIONS_ARGS,
        )?;
        parse_sessions_output(&output.stdout).map_err(RuntimeDiscoveryError::Parse)
    }

    pub(crate) fn list_tabs(&self) -> Result<Vec<TabInfo>, RuntimeDiscoveryError> {
        self.list_tabs_with_args(
            "zellij action list-tabs --json --all".to_owned(),
            &LIST_TABS_ARGS,
        )
    }

    pub(crate) fn list_tabs_for_session(
        &self,
        session_name: &str,
    ) -> Result<Vec<TabInfo>, RuntimeDiscoveryError> {
        let command = format!("zellij --session {session_name} action list-tabs --json --all");
        let args = [
            "--session",
            session_name,
            "action",
            "list-tabs",
            "--json",
            "--all",
        ];
        self.list_tabs_with_args(command, &args)
    }

    pub(crate) fn list_panes(&self) -> Result<Vec<PaneInfo>, RuntimeDiscoveryError> {
        self.list_panes_with_args(
            "zellij action list-panes --json --all".to_owned(),
            &LIST_PANES_ARGS,
        )
    }

    pub(crate) fn list_panes_for_session(
        &self,
        session_name: &str,
    ) -> Result<Vec<PaneInfo>, RuntimeDiscoveryError> {
        let command = format!("zellij --session {session_name} action list-panes --json --all");
        let args = [
            "--session",
            session_name,
            "action",
            "list-panes",
            "--json",
            "--all",
        ];
        self.list_panes_with_args(command, &args)
    }

    pub(crate) fn dump_screen_for_pane(
        &self,
        session_name: &str,
        pane_id: u64,
    ) -> Result<PaneSnapshot, RuntimeDiscoveryError> {
        self.dump_screen_for_pane_with_cancel(session_name, pane_id, || false)
    }

    pub(crate) fn dump_screen_for_pane_with_cancel<F>(
        &self,
        session_name: &str,
        pane_id: u64,
        is_cancelled: F,
    ) -> Result<PaneSnapshot, RuntimeDiscoveryError>
    where
        F: Fn() -> bool,
    {
        let pane_id_arg = pane_id.to_string();
        let command = format!(
            "zellij --session {session_name} action dump-screen --pane-id {pane_id} --ansi"
        );
        let args = [
            "--session",
            session_name,
            "action",
            "dump-screen",
            "--pane-id",
            pane_id_arg.as_str(),
            "--ansi",
        ];

        let output = self.run_checked_with_cancel(command, &args, &is_cancelled)?;
        if is_empty_snapshot_output(&output.stdout) {
            Ok(PaneSnapshot::Empty)
        } else {
            Ok(PaneSnapshot::Ready(output.stdout))
        }
    }

    pub(crate) fn subscribe_pane_updates_with_cancel<F, C>(
        &self,
        session_name: &str,
        pane_id: u64,
        mut on_snapshot: F,
        is_cancelled: C,
    ) -> Result<(), RuntimeDiscoveryError>
    where
        F: FnMut(PaneSnapshot),
        C: Fn() -> bool,
    {
        let pane_id_arg = pane_id.to_string();
        let command = format!(
            "zellij --session {session_name} subscribe --pane-id {pane_id} --ansi --format json --scrollback 0"
        );
        let args = [
            "--session",
            session_name,
            "subscribe",
            "--pane-id",
            pane_id_arg.as_str(),
            "--ansi",
            "--format",
            "json",
            "--scrollback",
            "0",
        ];

        let parse_failed = AtomicBool::new(false);
        let mut parse_error = None;
        let output = match self.runner.run_with_cancellation_and_stdout_lines(
            ZELLIJ_PROGRAM,
            &args,
            &|| is_cancelled() || parse_failed.load(Ordering::Relaxed),
            &mut |line| {
                if line.trim().is_empty() || parse_error.is_some() {
                    return;
                }

                match parse_subscribe_update_line(line) {
                    Ok(Some(snapshot)) => on_snapshot(snapshot),
                    Ok(None) => {}
                    Err(error) => {
                        parse_error = Some(error);
                        parse_failed.store(true, Ordering::Relaxed);
                    }
                }
            },
        ) {
            Ok(output) => output,
            Err(CommandRunnerError::Cancelled) => {
                if let Some(error) = parse_error {
                    return Err(RuntimeDiscoveryError::Parse(error));
                }

                return Err(RuntimeDiscoveryError::Cancelled { command });
            }
            Err(other) => return Err(RuntimeDiscoveryError::CommandRunner(other)),
        };

        if output.exit_code != Some(0) {
            return Err(RuntimeDiscoveryError::CommandFailed {
                command,
                exit_code: output.exit_code,
                stderr: output.stderr,
            });
        }

        if let Some(error) = parse_error {
            if is_session_targeting_diagnostic(&output.stdout, &output.stderr) {
                return Err(RuntimeDiscoveryError::RuntimeDiagnostic {
                    command,
                    stdout: output.stdout,
                    stderr: output.stderr,
                });
            }

            return Err(RuntimeDiscoveryError::Parse(error));
        }

        if output.stdout.trim().is_empty()
            && is_session_targeting_diagnostic(&output.stdout, &output.stderr)
        {
            return Err(RuntimeDiscoveryError::RuntimeDiagnostic {
                command,
                stdout: output.stdout,
                stderr: output.stderr,
            });
        }

        Ok(())
    }

    fn list_tabs_with_args(
        &self,
        command_description: String,
        args: &[&str],
    ) -> Result<Vec<TabInfo>, RuntimeDiscoveryError> {
        let output = self.run_checked(command_description.clone(), args)?;
        if output.stdout.trim().is_empty()
            && is_session_targeting_diagnostic(&output.stdout, &output.stderr)
        {
            return Err(RuntimeDiscoveryError::RuntimeDiagnostic {
                command: command_description,
                stdout: output.stdout,
                stderr: output.stderr,
            });
        }
        parse_tabs_output(&output.stdout)
            .map_err(|error| Self::classify_parse_error(command_description, output, error))
    }

    fn list_panes_with_args(
        &self,
        command_description: String,
        args: &[&str],
    ) -> Result<Vec<PaneInfo>, RuntimeDiscoveryError> {
        let output = self.run_checked(command_description.clone(), args)?;
        if output.stdout.trim().is_empty()
            && is_session_targeting_diagnostic(&output.stdout, &output.stderr)
        {
            return Err(RuntimeDiscoveryError::RuntimeDiagnostic {
                command: command_description,
                stdout: output.stdout,
                stderr: output.stderr,
            });
        }
        parse_panes_output(&output.stdout)
            .map_err(|error| Self::classify_parse_error(command_description, output, error))
    }

    fn run_checked(
        &self,
        command_description: String,
        args: &[&str],
    ) -> Result<CommandOutput, RuntimeDiscoveryError> {
        self.run_checked_with_cancel(command_description, args, &|| false)
    }

    fn run_checked_with_cancel(
        &self,
        command_description: String,
        args: &[&str],
        is_cancelled: &dyn Fn() -> bool,
    ) -> Result<CommandOutput, RuntimeDiscoveryError> {
        let output = self
            .runner
            .run_with_cancellation(ZELLIJ_PROGRAM, args, is_cancelled)
            .map_err(|error| match error {
                CommandRunnerError::Cancelled => RuntimeDiscoveryError::Cancelled {
                    command: command_description.clone(),
                },
                other => RuntimeDiscoveryError::CommandRunner(other),
            })?;

        if output.exit_code == Some(0) {
            return Ok(output);
        }

        Err(RuntimeDiscoveryError::CommandFailed {
            command: command_description,
            exit_code: output.exit_code,
            stderr: output.stderr,
        })
    }

    fn classify_parse_error(
        command: String,
        output: CommandOutput,
        parse_error: ParseError,
    ) -> RuntimeDiscoveryError {
        match parse_error {
            ParseError::InvalidJson { .. }
                if is_session_targeting_diagnostic(&output.stdout, &output.stderr) =>
            {
                RuntimeDiscoveryError::RuntimeDiagnostic {
                    command,
                    stdout: output.stdout,
                    stderr: output.stderr,
                }
            }
            _ => RuntimeDiscoveryError::Parse(parse_error),
        }
    }
}

fn is_session_targeting_diagnostic(stdout: &str, stderr: &str) -> bool {
    let diagnostic = format!("{stdout}\n{stderr}").to_ascii_lowercase();
    diagnostic.contains("session")
        && (diagnostic.contains("session name")
            || diagnostic.contains("specify")
            || diagnostic.contains("--session"))
}

fn is_empty_snapshot_output(stdout: &str) -> bool {
    if stdout.trim().is_empty() {
        return true;
    }

    stdout
        .replace("\u{1b}[0m", "")
        .replace("\u{1b}[m", "")
        .trim()
        .is_empty()
}

#[derive(Debug, Deserialize)]
struct SubscribePaneUpdateLine {
    #[serde(default)]
    event: String,
    #[serde(default)]
    scrollback: Vec<String>,
    #[serde(default)]
    viewport: Vec<String>,
}

fn parse_subscribe_update_line(raw_line: &str) -> Result<Option<PaneSnapshot>, ParseError> {
    let update: SubscribePaneUpdateLine =
        serde_json::from_str(raw_line).map_err(|source| ParseError::InvalidJson {
            context: "subscribe",
            source,
        })?;

    if update.event != "pane_update" {
        return Ok(None);
    }

    let mut lines = update.scrollback;
    lines.extend(update.viewport);
    let body = lines.join("\n");

    if is_empty_snapshot_output(&body) {
        Ok(Some(PaneSnapshot::Empty))
    } else {
        Ok(Some(PaneSnapshot::Ready(body)))
    }
}

#[cfg(test)]
mod tests {
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
    };

    use super::*;

    type CommandCall = (String, Vec<String>);
    type RecordedCalls = Rc<RefCell<Vec<CommandCall>>>;

    #[derive(Clone)]
    struct RecordingRunner {
        calls: RecordedCalls,
        output: CommandOutput,
    }

    struct ParseFailFastRunner {
        observed_cancel: Rc<Cell<bool>>,
    }

    impl RecordingRunner {
        fn with_output(output: CommandOutput) -> Self {
            Self {
                calls: Rc::new(RefCell::new(Vec::new())),
                output,
            }
        }

        fn calls(&self) -> Vec<CommandCall> {
            self.calls.borrow().clone()
        }
    }

    impl CommandRunner for RecordingRunner {
        fn run(&self, program: &str, args: &[&str]) -> Result<CommandOutput, CommandRunnerError> {
            self.calls.borrow_mut().push((
                program.to_owned(),
                args.iter().map(|arg| (*arg).to_owned()).collect(),
            ));
            Ok(self.output.clone())
        }
    }

    impl CommandRunner for ParseFailFastRunner {
        fn run(&self, _program: &str, _args: &[&str]) -> Result<CommandOutput, CommandRunnerError> {
            Ok(CommandOutput {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: Some(0),
            })
        }

        fn run_with_cancellation_and_stdout_lines(
            &self,
            _program: &str,
            _args: &[&str],
            is_cancelled: &dyn Fn() -> bool,
            on_stdout_line: &mut dyn FnMut(&str),
        ) -> Result<CommandOutput, CommandRunnerError> {
            on_stdout_line("{\"event\":");

            if is_cancelled() {
                self.observed_cancel.set(true);
                return Err(CommandRunnerError::Cancelled);
            }

            Ok(CommandOutput {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: Some(0),
            })
        }
    }

    fn ok_json_output() -> CommandOutput {
        CommandOutput {
            stdout: "[]".to_owned(),
            stderr: String::new(),
            exit_code: Some(0),
        }
    }

    #[test]
    fn list_tabs_for_session_uses_scoped_command_args() {
        let runner = RecordingRunner::with_output(ok_json_output());
        let discovery = RuntimeDiscovery::new(runner.clone());

        let tabs = discovery
            .list_tabs_for_session("work")
            .expect("scoped list-tabs should parse");

        assert!(tabs.is_empty());
        assert_eq!(
            runner.calls(),
            vec![(
                "zellij".to_owned(),
                vec![
                    "--session".to_owned(),
                    "work".to_owned(),
                    "action".to_owned(),
                    "list-tabs".to_owned(),
                    "--json".to_owned(),
                    "--all".to_owned(),
                ],
            )]
        );
    }

    #[test]
    fn list_panes_for_session_uses_scoped_command_args() {
        let runner = RecordingRunner::with_output(ok_json_output());
        let discovery = RuntimeDiscovery::new(runner.clone());

        let panes = discovery
            .list_panes_for_session("work")
            .expect("scoped list-panes should parse");

        assert!(panes.is_empty());
        assert_eq!(
            runner.calls(),
            vec![(
                "zellij".to_owned(),
                vec![
                    "--session".to_owned(),
                    "work".to_owned(),
                    "action".to_owned(),
                    "list-panes".to_owned(),
                    "--json".to_owned(),
                    "--all".to_owned(),
                ],
            )]
        );
    }

    #[test]
    fn dump_screen_for_pane_uses_scoped_command_args() {
        let runner = RecordingRunner::with_output(CommandOutput {
            stdout: "pane output".to_owned(),
            stderr: String::new(),
            exit_code: Some(0),
        });
        let discovery = RuntimeDiscovery::new(runner.clone());

        let snapshot = discovery
            .dump_screen_for_pane("work", 42)
            .expect("dump-screen should run for scoped pane");

        assert_eq!(snapshot, PaneSnapshot::Ready("pane output".to_owned()));
        assert_eq!(
            runner.calls(),
            vec![(
                "zellij".to_owned(),
                vec![
                    "--session".to_owned(),
                    "work".to_owned(),
                    "action".to_owned(),
                    "dump-screen".to_owned(),
                    "--pane-id".to_owned(),
                    "42".to_owned(),
                    "--ansi".to_owned(),
                ],
            )]
        );
    }

    #[test]
    fn dump_screen_treats_empty_and_reset_only_output_as_empty_state() {
        let runner = RecordingRunner::with_output(CommandOutput {
            stdout: "\u{1b}[0m\n\u{1b}[m".to_owned(),
            stderr: String::new(),
            exit_code: Some(0),
        });
        let discovery = RuntimeDiscovery::new(runner);

        let snapshot = discovery
            .dump_screen_for_pane("work", 3)
            .expect("reset-only output should map to empty snapshot");

        assert_eq!(snapshot, PaneSnapshot::Empty);
    }

    #[test]
    fn dump_screen_non_zero_exit_is_command_failed_error() {
        let runner = RecordingRunner::with_output(CommandOutput {
            stdout: String::new(),
            stderr: "pane missing".to_owned(),
            exit_code: Some(1),
        });
        let discovery = RuntimeDiscovery::new(runner);

        let error = discovery
            .dump_screen_for_pane("work", 3)
            .expect_err("non-zero exit should be surfaced as command failure");

        match error {
            RuntimeDiscoveryError::CommandFailed {
                command,
                exit_code,
                stderr,
            } => {
                assert_eq!(
                    command,
                    "zellij --session work action dump-screen --pane-id 3 --ansi"
                );
                assert_eq!(exit_code, Some(1));
                assert_eq!(stderr, "pane missing");
            }
            other => panic!("unexpected error variant: {other:?}"),
        }
    }

    #[test]
    fn non_json_exit_zero_session_prompt_is_runtime_diagnostic() {
        let runner = RecordingRunner::with_output(CommandOutput {
            stdout: "Please specify a session name".to_owned(),
            stderr: "Hint: pass --session <name>".to_owned(),
            exit_code: Some(0),
        });
        let discovery = RuntimeDiscovery::new(runner);

        let error = discovery
            .list_tabs()
            .expect_err("session-targeting diagnostic should not be a parse error");

        match error {
            RuntimeDiscoveryError::RuntimeDiagnostic {
                command,
                stdout,
                stderr,
            } => {
                assert_eq!(command, "zellij action list-tabs --json --all");
                assert!(stdout.contains("session name"));
                assert!(stderr.contains("--session"));
            }
            other => panic!("unexpected error variant: {other:?}"),
        }
    }

    #[test]
    fn stderr_only_exit_zero_session_prompt_is_runtime_diagnostic_for_tabs() {
        let runner = RecordingRunner::with_output(CommandOutput {
            stdout: String::new(),
            stderr: "Please specify a session name via --session".to_owned(),
            exit_code: Some(0),
        });
        let discovery = RuntimeDiscovery::new(runner);

        let error = discovery
            .list_tabs()
            .expect_err("stderr-only session prompt should not be parsed as empty tabs");

        match error {
            RuntimeDiscoveryError::RuntimeDiagnostic {
                command,
                stdout,
                stderr,
            } => {
                assert_eq!(command, "zellij action list-tabs --json --all");
                assert!(stdout.is_empty());
                assert!(stderr.contains("session name"));
            }
            other => panic!("unexpected error variant: {other:?}"),
        }
    }

    #[test]
    fn stderr_only_exit_zero_session_prompt_is_runtime_diagnostic_for_panes() {
        let runner = RecordingRunner::with_output(CommandOutput {
            stdout: String::new(),
            stderr: "Please specify a session name via --session".to_owned(),
            exit_code: Some(0),
        });
        let discovery = RuntimeDiscovery::new(runner);

        let error = discovery
            .list_panes()
            .expect_err("stderr-only session prompt should not be parsed as empty panes");

        match error {
            RuntimeDiscoveryError::RuntimeDiagnostic {
                command,
                stdout,
                stderr,
            } => {
                assert_eq!(command, "zellij action list-panes --json --all");
                assert!(stdout.is_empty());
                assert!(stderr.contains("session name"));
            }
            other => panic!("unexpected error variant: {other:?}"),
        }
    }

    #[test]
    fn dump_screen_with_cancel_maps_cancelled_runner_error() {
        let runner = RecordingRunner::with_output(CommandOutput {
            stdout: "pane output".to_owned(),
            stderr: String::new(),
            exit_code: Some(0),
        });
        let discovery = RuntimeDiscovery::new(runner.clone());

        let error = discovery
            .dump_screen_for_pane_with_cancel("work", 8, || true)
            .expect_err("cancelled dump-screen should not return a snapshot");

        match error {
            RuntimeDiscoveryError::Cancelled { command } => {
                assert_eq!(
                    command,
                    "zellij --session work action dump-screen --pane-id 8 --ansi"
                );
            }
            other => panic!("unexpected error variant: {other:?}"),
        }

        assert!(runner.calls().is_empty());
    }

    #[test]
    fn parse_subscribe_update_line_joins_scrollback_and_viewport() {
        let line =
            r#"{"event":"pane_update","scrollback":["line-1"],"viewport":["line-2","line-3"]}"#;

        let parsed = parse_subscribe_update_line(line).expect("subscribe line should parse");
        assert_eq!(
            parsed,
            Some(PaneSnapshot::Ready("line-1\nline-2\nline-3".to_owned()))
        );
    }

    #[test]
    fn parse_subscribe_update_line_maps_reset_only_content_to_empty() {
        let line = r#"{"event":"pane_update","scrollback":[],"viewport":["\u001b[0m","\u001b[m"]}"#;

        let parsed = parse_subscribe_update_line(line).expect("subscribe line should parse");
        assert_eq!(parsed, Some(PaneSnapshot::Empty));
    }

    #[test]
    fn subscribe_pane_updates_uses_scoped_command_args_and_parses_frames() {
        let runner = RecordingRunner::with_output(CommandOutput {
            stdout: concat!(
                "{\"event\":\"session_update\"}\n",
                "{\"event\":\"pane_update\",\"scrollback\":[],\"viewport\":[\"frame-1\"]}\n",
                "{\"event\":\"pane_update\",\"scrollback\":[\"a\"],\"viewport\":[\"b\"]}\n"
            )
            .to_owned(),
            stderr: String::new(),
            exit_code: Some(0),
        });
        let discovery = RuntimeDiscovery::new(runner.clone());
        let mut snapshots = Vec::new();

        discovery
            .subscribe_pane_updates_with_cancel(
                "work",
                9,
                |snapshot| snapshots.push(snapshot),
                || false,
            )
            .expect("subscribe should parse pane updates");

        assert_eq!(
            snapshots,
            vec![
                PaneSnapshot::Ready("frame-1".to_owned()),
                PaneSnapshot::Ready("a\nb".to_owned())
            ]
        );
        assert_eq!(
            runner.calls(),
            vec![(
                "zellij".to_owned(),
                vec![
                    "--session".to_owned(),
                    "work".to_owned(),
                    "subscribe".to_owned(),
                    "--pane-id".to_owned(),
                    "9".to_owned(),
                    "--ansi".to_owned(),
                    "--format".to_owned(),
                    "json".to_owned(),
                    "--scrollback".to_owned(),
                    "0".to_owned(),
                ],
            )]
        );
    }

    #[test]
    fn subscribe_pane_updates_non_zero_exit_is_command_failed_error() {
        let runner = RecordingRunner::with_output(CommandOutput {
            stdout: String::new(),
            stderr: "subscribe failed".to_owned(),
            exit_code: Some(1),
        });
        let discovery = RuntimeDiscovery::new(runner);

        let error = discovery
            .subscribe_pane_updates_with_cancel("work", 9, |_| {}, || false)
            .expect_err("non-zero subscribe exit should be surfaced");

        match error {
            RuntimeDiscoveryError::CommandFailed {
                command,
                exit_code,
                stderr,
            } => {
                assert_eq!(
                    command,
                    "zellij --session work subscribe --pane-id 9 --ansi --format json --scrollback 0"
                );
                assert_eq!(exit_code, Some(1));
                assert_eq!(stderr, "subscribe failed");
            }
            other => panic!("unexpected error variant: {other:?}"),
        }
    }

    #[test]
    fn subscribe_pane_updates_parse_error_cancels_runner_and_returns_parse_error() {
        let observed_cancel = Rc::new(Cell::new(false));
        let discovery = RuntimeDiscovery::new(ParseFailFastRunner {
            observed_cancel: Rc::clone(&observed_cancel),
        });

        let error = discovery
            .subscribe_pane_updates_with_cancel("work", 9, |_| {}, || false)
            .expect_err("malformed subscribe frame should fail fast");

        assert!(matches!(
            error,
            RuntimeDiscoveryError::Parse(ParseError::InvalidJson { context, .. })
                if context == "subscribe"
        ));
        assert!(observed_cancel.get());
    }
}
