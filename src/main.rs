#[allow(dead_code)]
mod navigation_model;
mod runtime_discovery;
mod tui;

use runtime_discovery::{RuntimeDiscovery, SystemCommandRunner, command_contract};

fn main() {
    let mode = std::env::args().nth(1);
    match mode.as_deref() {
        Some("status") => print_status(),
        Some("demo") => run_demo(),
        Some("--help") | Some("-h") => print_help(),
        _ => print_placeholder(),
    }
}

fn print_placeholder() {
    println!("zellijess TUI shell (M5 selected-pane near-live preview) is ready");
    println!("run `zellijess demo` for fixture-backed TUI mode");
    println!("run `zellijess status` to probe runtime discovery commands");
}

fn print_help() {
    println!("zellijess (M5 selected-pane near-live preview shell)");
    println!();
    println!("commands:");
    println!("  demo     run fixture-backed static TUI shell");
    println!("  status   run read-only probes (session-scoped + current-context fallback)");
    println!("  --help   print this message");
    println!();
    println!("runtime command contract (reliable path is session-scoped):");
    for command in command_contract() {
        println!("  - {command}");
    }
}

fn run_demo() {
    if let Err(error) = tui::run_static_demo() {
        eprintln!("failed to run demo TUI: {error}");
        std::process::exit(1);
    }
}

fn print_status() {
    let discovery = RuntimeDiscovery::new(SystemCommandRunner);

    println!("zellijess runtime-discovery status");
    println!("  reliable tab/pane discovery uses --session <session>");

    match discovery.list_sessions() {
        Ok(sessions) => {
            println!("  list-sessions: ok ({} records)", sessions.len());

            if let Some(session) = sessions.first() {
                let session_name = session.name.as_str();
                let tabs_label = format!("list-tabs --session {session_name}");
                probe(&tabs_label, || {
                    discovery
                        .list_tabs_for_session(session_name)
                        .map(|tabs| tabs.len())
                });

                let panes_label = format!("list-panes --session {session_name}");
                probe(&panes_label, || {
                    discovery
                        .list_panes_for_session(session_name)
                        .map(|panes| panes.len())
                });
            } else {
                println!("  list-tabs --session <session>: skipped (no sessions discovered)");
                println!("  list-panes --session <session>: skipped (no sessions discovered)");
            }
        }
        Err(error) => {
            println!("  list-sessions: unavailable ({error})");
            println!("  list-tabs --session <session>: skipped (session discovery unavailable)");
            println!("  list-panes --session <session>: skipped (session discovery unavailable)");
        }
    }

    probe("list-tabs (unscoped best-effort)", || {
        discovery.list_tabs().map(|tabs| tabs.len())
    });
    probe("list-panes (unscoped best-effort)", || {
        discovery.list_panes().map(|panes| panes.len())
    });
}

fn probe<F>(label: &str, probe_fn: F)
where
    F: FnOnce() -> Result<usize, runtime_discovery::RuntimeDiscoveryError>,
{
    match probe_fn() {
        Ok(count) => println!("  {label}: ok ({count} records)"),
        Err(error) => println!("  {label}: unavailable ({error})"),
    }
}
