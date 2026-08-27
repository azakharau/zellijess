use ansi_to_tui::IntoText;
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};

use crate::navigation_model::{PreviewTarget, SelectionAction, SourceFreshness, VisibleItem};

use super::palette;
use super::preview::{PaneSnapshotRequest, SnapshotPreviewState};
use super::state::{AppState, InputMode};

pub(crate) fn render(frame: &mut Frame, state: &AppState) {
    let root_area = frame.area();
    frame.render_widget(
        Block::default().style(Style::default().bg(palette::BG)),
        root_area,
    );

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(root_area);
    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(46), Constraint::Percentage(54)])
        .split(layout[0]);

    let visible_items = state.visible_items();
    render_navigation_panel(
        frame,
        body[0],
        &visible_items,
        state.selected_index(visible_items.len()),
    );
    render_preview_panel(frame, body[1], state);
    render_status_line(frame, layout[1], state, visible_items.len());
}

fn render_navigation_panel(
    frame: &mut Frame,
    area: Rect,
    visible_items: &[VisibleItem],
    selected_index: Option<usize>,
) {
    if visible_items.is_empty() {
        let empty_state = Paragraph::new("No rows match current filter")
            .style(Style::default().fg(palette::MUTED).bg(palette::BG))
            .block(
                Block::default()
                    .title(" navigation ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(palette::MUTED)),
            );
        frame.render_widget(empty_state, area);
        return;
    }

    let items = visible_items
        .iter()
        .map(|item| {
            let indent = "  ".repeat(item.depth);
            let marker = if item.selectable { "•" } else { "·" };
            let line = format!("{indent}{marker} {}", item.label);
            let style = if item.selectable {
                Style::default().fg(palette::FG)
            } else {
                Style::default().fg(palette::MUTED)
            };
            ListItem::new(line).style(style)
        })
        .collect::<Vec<_>>();

    let mut list_state = ListState::default();
    list_state.select(selected_index);

    let list = List::new(items)
        .block(
            Block::default()
                .title(" navigation ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(palette::MUTED)),
        )
        .highlight_symbol("› ")
        .highlight_style(
            Style::default()
                .bg(palette::ACCENT_MUTED_BG)
                .fg(palette::ACCENT)
                .add_modifier(Modifier::BOLD),
        );

    frame.render_stateful_widget(list, area, &mut list_state);
}

fn render_preview_panel(frame: &mut Frame, area: Rect, state: &AppState) {
    let preview_target = state.current_preview_target();
    let preview_lines =
        preview_target_lines(&preview_target, state.current_snapshot_preview_state());
    let preview = Paragraph::new(preview_lines)
        .style(Style::default().fg(palette::FG).bg(palette::BG))
        .block(
            Block::default()
                .title(" preview ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(palette::MUTED)),
        );

    frame.render_widget(preview, area);
}

fn preview_target_lines(
    preview_target: &PreviewTarget,
    snapshot_preview_state: &SnapshotPreviewState,
) -> Vec<Line<'static>> {
    match preview_target {
        PreviewTarget::SessionSummary {
            session_name,
            source_freshness,
        } => vec![
            Line::from("Session summary"),
            Line::from(format!("session: {session_name}")),
            Line::from(format!("source: {}", freshness_label(*source_freshness))),
            Line::from(""),
            Line::from("select a terminal pane to capture a snapshot"),
        ],
        PreviewTarget::TabSummary {
            session_name,
            tab_id,
            tab_position,
            source_freshness,
        } => vec![
            Line::from("Tab summary"),
            Line::from(format!("session: {session_name}")),
            Line::from(format!("tab: id={tab_id}, position={tab_position}")),
            Line::from(format!("source: {}", freshness_label(*source_freshness))),
            Line::from(""),
            Line::from("select a terminal pane to capture a snapshot"),
        ],
        PreviewTarget::PaneSnapshotCandidate {
            session_name,
            tab_id,
            tab_position,
            pane_id,
            source_freshness,
        } => {
            let target_request = PaneSnapshotRequest {
                session_name: session_name.clone(),
                tab_id: *tab_id,
                tab_position: *tab_position,
                pane_id: *pane_id,
            };

            match snapshot_preview_state {
                SnapshotPreviewState::Loading { request } => pane_snapshot_lines(
                    "Pane snapshot (loading)",
                    request,
                    vec![Line::from(
                        "fetching initial dump-screen output before near-live refresh...",
                    )],
                ),
                SnapshotPreviewState::Ready { request, body } => {
                    let rendered_snapshot_body = render_snapshot_body_lines(body);
                    let rendered_line_count = rendered_snapshot_body.lines.len();
                    let mut content_lines = vec![Line::from("snapshot body:")];
                    content_lines.extend(rendered_snapshot_body.lines);
                    if body.trim().is_empty() {
                        content_lines.push(Line::from("(snapshot was empty after trim)"));
                    } else if rendered_line_count == 0 {
                        content_lines.push(Line::from(
                            "(snapshot had no printable content after ANSI sanitize)",
                        ));
                    }
                    if let Some(note) = rendered_snapshot_body.note {
                        content_lines.push(Line::from(""));
                        content_lines.push(Line::from(note));
                    }
                    pane_snapshot_lines("Pane snapshot", request, content_lines)
                }
                SnapshotPreviewState::Empty { request } => pane_snapshot_lines(
                    "Pane snapshot (empty)",
                    request,
                    vec![Line::from(
                        "no visible pane content returned by dump-screen (near-live refresh continues)",
                    )],
                ),
                SnapshotPreviewState::Error { request, message } => pane_snapshot_lines(
                    "Pane snapshot (error)",
                    request,
                    sanitized_error_lines(message),
                ),
                SnapshotPreviewState::Stale { request } => pane_snapshot_lines(
                    "Pane snapshot (stale)",
                    request,
                    vec![Line::from(
                        "selection source is stale; move selection to refresh",
                    )],
                ),
                SnapshotPreviewState::Unavailable { reason } => pane_snapshot_lines(
                    "Pane snapshot unavailable",
                    &target_request,
                    vec![
                        Line::from(format!("source: {}", freshness_label(*source_freshness))),
                        Line::from(reason.clone()),
                    ],
                ),
            }
        }
        PreviewTarget::Unavailable { reason } => vec![
            Line::from("Preview unavailable"),
            Line::from(format!("reason: {reason}")),
            Line::from(""),
            Line::from("selection has no preview route"),
        ],
    }
}

#[derive(Debug, PartialEq, Eq)]
struct SnapshotBodyLines {
    lines: Vec<Line<'static>>,
    note: Option<String>,
}

fn render_snapshot_body_lines(body: &str) -> SnapshotBodyLines {
    snapshot_body_lines_from_parse_result(body, parse_ansi_snapshot_body_lines(body))
}

fn parse_ansi_snapshot_body_lines(body: &str) -> Result<Vec<Line<'static>>, String> {
    body.into_text()
        .map(|text| text.lines.into_iter().map(|line| line.to_owned()).collect())
        .map_err(|error| error.to_string())
}

fn snapshot_body_lines_from_parse_result(
    body: &str,
    parse_result: Result<Vec<Line<'static>>, String>,
) -> SnapshotBodyLines {
    match parse_result {
        Ok(lines) => SnapshotBodyLines {
            lines: sanitize_parsed_ansi_lines(lines),
            note: None,
        },
        Err(error) => SnapshotBodyLines {
            lines: plain_snapshot_body_lines(body),
            note: Some(format!(
                "note: ANSI parse failed; showing sanitized plain text ({error})"
            )),
        },
    }
}

fn sanitize_parsed_ansi_lines(mut lines: Vec<Line<'static>>) -> Vec<Line<'static>> {
    for line in &mut lines {
        for span in &mut line.spans {
            if span.content.contains('\u{1b}') {
                span.content = strip_ansi_escape_sequences(span.content.as_ref()).into();
            }
        }
    }
    lines
}

fn plain_snapshot_body_lines(body: &str) -> Vec<Line<'static>> {
    strip_ansi_escape_sequences(body)
        .lines()
        .map(|line| Line::from(line.to_owned()))
        .collect()
}

fn sanitized_error_lines(message: &str) -> Vec<Line<'static>> {
    let sanitized_message = strip_ansi_escape_sequences(message);
    if sanitized_message.is_empty() {
        return vec![Line::from("(no error details available)")];
    }

    sanitized_message
        .lines()
        .map(|line| Line::from(line.to_owned()))
        .collect()
}

fn strip_ansi_escape_sequences(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(character) = chars.next() {
        if character != '\u{1b}' {
            output.push(character);
            continue;
        }

        match chars.peek().copied() {
            Some('[') => {
                let _ = chars.next();
                for next in chars.by_ref() {
                    if ('@'..='~').contains(&next) {
                        break;
                    }
                }
            }
            Some(']') => {
                let _ = chars.next();
                loop {
                    match chars.next() {
                        Some('\u{07}') => break,
                        Some('\u{1b}') if chars.peek().copied() == Some('\\') => {
                            let _ = chars.next();
                            break;
                        }
                        Some(_) => {}
                        None => break,
                    }
                }
            }
            Some(_) => {
                let _ = chars.next();
            }
            None => break,
        }
    }

    output
}

fn pane_snapshot_lines(
    title: &str,
    request: &PaneSnapshotRequest,
    details: Vec<Line<'static>>,
) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(title.to_owned()),
        Line::from(format!("session: {}", request.session_name)),
        Line::from(format!(
            "tab: id={}, position={}",
            request.tab_id, request.tab_position
        )),
        Line::from(format!("pane: id={}", request.pane_id)),
        Line::from(""),
    ];
    lines.extend(details);
    lines
}

fn freshness_label(freshness: SourceFreshness) -> &'static str {
    match freshness {
        SourceFreshness::Runtime => "runtime",
        SourceFreshness::Subscription => "subscription",
        SourceFreshness::Cache => "cache",
        SourceFreshness::Stale => "stale",
        SourceFreshness::Error => "error",
    }
}

fn render_status_line(frame: &mut Frame, area: Rect, state: &AppState, visible_count: usize) {
    let mode = match state.mode() {
        InputMode::Navigate => "NAV",
        InputMode::Filter => "FILTER",
    };
    let filter = if state.filter_query().is_empty() {
        "-".to_owned()
    } else {
        format!("/{}", state.filter_query())
    };
    let pending = state
        .pending_action()
        .map(selection_action_summary)
        .unwrap_or_else(|| "none".to_owned());

    let status = format!("{mode} rows:{visible_count} filter:{filter} pending:{pending}");
    let style = if matches!(state.pending_action(), Some(SelectionAction::NoAction)) {
        Style::default().fg(palette::WARN).bg(palette::BG)
    } else if state.pending_action().is_some() {
        Style::default().fg(palette::SUCCESS).bg(palette::BG)
    } else {
        Style::default().fg(palette::FG).bg(palette::BG)
    };

    frame.render_widget(Paragraph::new(status).style(style), area);
}

fn selection_action_summary(action: &SelectionAction) -> String {
    match action {
        SelectionAction::SwitchSession { session_name } => {
            format!("switch-session {session_name}")
        }
        SelectionAction::SwitchTab {
            session_name,
            tab_position,
            ..
        } => format!("switch-tab {session_name}@{tab_position}"),
        SelectionAction::FocusPane {
            session_name,
            tab_position,
            pane_id,
            ..
        } => format!("focus-pane {session_name}@{tab_position}:{pane_id}"),
        SelectionAction::NoAction => "no-action".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::style::Color;
    use ratatui::text::Line;

    use crate::runtime_discovery::{PaneSnapshot, RuntimeDiscoveryError};
    use crate::tui::demo_data::load_navigation_model;
    use crate::tui::preview::{PaneSnapshotRequest, SnapshotLoader};
    use crate::tui::state::AppState;

    use super::{
        render, render_snapshot_body_lines, sanitized_error_lines,
        snapshot_body_lines_from_parse_result,
    };

    #[test]
    fn render_smoke_draws_on_regular_and_small_terminals() {
        let model = load_navigation_model().expect("fixture model should load");
        let state = AppState::new(model);

        let mut wide_terminal =
            Terminal::new(TestBackend::new(90, 26)).expect("wide terminal should initialize");
        wide_terminal
            .draw(|frame| render(frame, &state))
            .expect("render should succeed on wide terminal");

        let mut small_terminal =
            Terminal::new(TestBackend::new(38, 10)).expect("small terminal should initialize");
        small_terminal
            .draw(|frame| render(frame, &state))
            .expect("render should succeed on small terminal");
    }

    #[derive(Debug, Clone, Copy)]
    enum LoaderMode {
        Ready,
        Empty,
        Error,
        AnsiReady,
    }

    #[derive(Debug, Clone, Copy)]
    struct StaticLoader {
        mode: LoaderMode,
    }

    impl SnapshotLoader for StaticLoader {
        fn load_snapshot(
            &self,
            _request: &PaneSnapshotRequest,
        ) -> Result<PaneSnapshot, RuntimeDiscoveryError> {
            match self.mode {
                LoaderMode::Ready => Ok(PaneSnapshot::Ready("pane-line".to_owned())),
                LoaderMode::Empty => Ok(PaneSnapshot::Empty),
                LoaderMode::Error => Err(RuntimeDiscoveryError::CommandFailed {
                    command: "zellij --session zellijess action dump-screen --pane-id 3 --ansi"
                        .to_owned(),
                    exit_code: Some(1),
                    stderr: "\x1b[31mfailed to fetch pane snapshot\x1b[0m".to_owned(),
                }),
                LoaderMode::AnsiReady => Ok(PaneSnapshot::Ready("\x1b[31mred\x1b[0m".to_owned())),
            }
        }
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn render_smoke_for_ready_empty_and_error_snapshot_states() {
        for mode in [LoaderMode::Ready, LoaderMode::Empty, LoaderMode::Error] {
            let model = load_navigation_model().expect("fixture model should load");
            let mut state =
                AppState::new_with_snapshot_loader(model, Box::new(StaticLoader { mode }));

            assert_eq!(
                state.handle_key_event(key(KeyCode::Down)),
                crate::tui::state::EventResult::Continue
            );
            assert_eq!(
                state.handle_key_event(key(KeyCode::Down)),
                crate::tui::state::EventResult::Continue
            );

            let mut terminal =
                Terminal::new(TestBackend::new(90, 26)).expect("terminal should initialize");
            terminal
                .draw(|frame| render(frame, &state))
                .expect("render should succeed for snapshot preview states");
        }
    }

    #[test]
    fn render_snapshot_body_lines_preserve_ansi_red_style_without_raw_escape() {
        let rendered = render_snapshot_body_lines("\x1b[31mred\x1b[0m");

        assert_eq!(rendered.note, None);
        assert_eq!(rendered.lines.len(), 1);
        let first_span = &rendered.lines[0].spans[0];
        assert_eq!(first_span.content.as_ref(), "red");
        assert_eq!(first_span.style.fg, Some(Color::Red));
        assert!(
            !rendered
                .lines
                .iter()
                .flat_map(|line| line.spans.iter())
                .any(|span| span.content.contains('\u{1b}'))
        );
    }

    #[test]
    fn forced_parse_error_sanitizes_body_and_returns_non_blocking_note() {
        let rendered = snapshot_body_lines_from_parse_result(
            "\x1b[31mred\x1b[0m",
            Err("forced parse failure".to_owned()),
        );

        assert_eq!(
            rendered.note,
            Some(
                "note: ANSI parse failed; showing sanitized plain text (forced parse failure)"
                    .to_owned()
            )
        );
        assert_eq!(rendered.lines, vec![Line::from("red")]);
    }

    #[test]
    fn successful_parse_result_still_strips_raw_escape_sequences() {
        let rendered = snapshot_body_lines_from_parse_result(
            "unused when parse succeeds",
            Ok(vec![Line::from("\x1b[31mred\x1b[0m")]),
        );

        assert_eq!(rendered.note, None);
        assert_eq!(rendered.lines, vec![Line::from("red")]);
        assert!(
            !rendered
                .lines
                .iter()
                .flat_map(|line| line.spans.iter())
                .any(|span| span.content.contains('\u{1b}'))
        );
    }

    #[test]
    fn render_smoke_ansi_snapshot_has_no_raw_escape_in_buffer() {
        let model = load_navigation_model().expect("fixture model should load");
        let mut state = AppState::new_with_snapshot_loader(
            model,
            Box::new(StaticLoader {
                mode: LoaderMode::AnsiReady,
            }),
        );

        assert_eq!(
            state.handle_key_event(key(KeyCode::Down)),
            crate::tui::state::EventResult::Continue
        );
        assert_eq!(
            state.handle_key_event(key(KeyCode::Down)),
            crate::tui::state::EventResult::Continue
        );

        let mut terminal =
            Terminal::new(TestBackend::new(90, 26)).expect("terminal should initialize");
        terminal
            .draw(|frame| render(frame, &state))
            .expect("render should succeed for ANSI snapshot preview");

        let buffer = terminal.backend().buffer();
        let mut rendered_text = String::new();
        for row in 0..buffer.area.height {
            for column in 0..buffer.area.width {
                rendered_text.push_str(buffer[(column, row)].symbol());
            }
        }

        assert!(rendered_text.contains("red"));
        assert!(!rendered_text.contains('\u{1b}'));
        assert!((0..buffer.area.height).any(|row| {
            (0..buffer.area.width).any(|column| {
                let cell = &buffer[(column, row)];
                cell.symbol() == "r" && cell.fg == Color::Red
            })
        }));
    }

    #[test]
    fn render_smoke_error_snapshot_has_no_raw_escape_in_buffer() {
        let model = load_navigation_model().expect("fixture model should load");
        let mut state = AppState::new_with_snapshot_loader(
            model,
            Box::new(StaticLoader {
                mode: LoaderMode::Error,
            }),
        );

        assert_eq!(
            state.handle_key_event(key(KeyCode::Down)),
            crate::tui::state::EventResult::Continue
        );
        assert_eq!(
            state.handle_key_event(key(KeyCode::Down)),
            crate::tui::state::EventResult::Continue
        );

        let mut terminal =
            Terminal::new(TestBackend::new(90, 26)).expect("terminal should initialize");
        terminal
            .draw(|frame| render(frame, &state))
            .expect("render should succeed for ANSI error preview");

        let buffer = terminal.backend().buffer();
        let mut rendered_text = String::new();
        for row in 0..buffer.area.height {
            for column in 0..buffer.area.width {
                rendered_text.push_str(buffer[(column, row)].symbol());
            }
        }

        assert!(rendered_text.contains("Pane snapshot (error)"));
        assert!(rendered_text.contains("zellij --session zellijess"));
        assert!(!rendered_text.contains('\u{1b}'));
    }

    #[test]
    fn sanitized_error_lines_strip_escape_sequences() {
        let lines = sanitized_error_lines("\x1b[31mboom\x1b[0m\nnext");
        assert_eq!(lines, vec![Line::from("boom"), Line::from("next")]);

        let empty_lines = sanitized_error_lines("\x1b[0m\x1b[m");
        assert_eq!(
            empty_lines,
            vec![Line::from("(no error details available)")]
        );
    }
}
