mod demo_data;
mod palette;
mod preview;
mod render;
mod state;

use std::io;
use std::io::IsTerminal;
use std::time::Duration;

use crossterm::event::{self, Event};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use crate::runtime_discovery::{RuntimeDiscovery, SystemCommandRunner};

use state::EventResult;

const EVENT_POLL_INTERVAL: Duration = Duration::from_millis(80);

#[derive(Debug, Default)]
struct TerminalModeGuard {
    raw_mode_enabled: bool,
    alternate_screen_enabled: bool,
    restored: bool,
}

impl TerminalModeGuard {
    fn activate() -> io::Result<Self> {
        enable_raw_mode()?;

        let mut guard = Self {
            raw_mode_enabled: true,
            ..Self::default()
        };

        let mut stdout = io::stdout();
        if let Err(error) = execute!(stdout, EnterAlternateScreen) {
            guard.restore_best_effort();
            return Err(error);
        }

        guard.alternate_screen_enabled = true;
        Ok(guard)
    }

    fn restore(&mut self) -> io::Result<()> {
        let leave_screen_result = if self.alternate_screen_enabled {
            let mut stdout = io::stdout();
            execute!(stdout, LeaveAlternateScreen)
        } else {
            Ok(())
        };

        let disable_raw_result = if self.raw_mode_enabled {
            disable_raw_mode()
        } else {
            Ok(())
        };

        self.alternate_screen_enabled = false;
        self.raw_mode_enabled = false;
        self.restored = true;

        leave_screen_result?;
        disable_raw_result
    }

    fn restore_best_effort(&mut self) {
        let _ = self.restore();
    }
}

impl Drop for TerminalModeGuard {
    fn drop(&mut self) {
        if !self.restored {
            self.restore_best_effort();
        }
    }
}

pub(crate) fn run_static_demo() -> io::Result<()> {
    let model = demo_data::load_navigation_model()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;

    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        println!("demo TUI requires an interactive terminal; fixture model loaded, skipping loop");
        return Ok(());
    }

    let snapshot_loader = Box::new(RuntimeDiscovery::new(SystemCommandRunner));
    let mut state = state::AppState::new_with_snapshot_loader(model, snapshot_loader);

    let mut mode_guard = TerminalModeGuard::activate()?;

    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;

    let run_result = run_loop(&mut terminal, &mut state);
    let show_cursor_result = terminal.show_cursor();
    drop(terminal);
    let restore_result = mode_guard.restore();

    run_result?;
    show_cursor_result?;
    restore_result
}

fn run_loop<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    state: &mut state::AppState,
) -> io::Result<()> {
    let mut needs_draw = true;

    loop {
        if needs_draw {
            terminal.draw(|frame| render::render(frame, state))?;
            needs_draw = false;
        }

        if event::poll(EVENT_POLL_INTERVAL)? {
            if let Event::Key(key_event) = event::read()?
                && state.handle_key_event(key_event) == EventResult::Quit
            {
                return Ok(());
            }
            needs_draw = true;
        }

        if state.poll_preview_updates() {
            needs_draw = true;
        }
    }
}
