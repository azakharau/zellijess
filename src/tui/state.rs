use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::navigation_model::{
    NavigationModel, NodeId, PreviewTarget, SelectionAction, VisibleItem,
};

use super::preview::{SnapshotLoader, SnapshotPreviewController, SnapshotPreviewState};

const NO_FILTER_RESULTS_REASON: &str = "no rows match current filter";
const NO_SELECTION_REASON: &str = "no selection available";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InputMode {
    Navigate,
    Filter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EventResult {
    Continue,
    Quit,
}

pub(crate) struct AppState {
    model: NavigationModel,
    mode: InputMode,
    filter_query: String,
    selected_index: usize,
    pending_action: Option<SelectionAction>,
    snapshot_preview: SnapshotPreviewController,
}

impl AppState {
    pub(crate) fn new(model: NavigationModel) -> Self {
        let mut state = Self {
            model,
            mode: InputMode::Navigate,
            filter_query: String::new(),
            selected_index: 0,
            pending_action: None,
            snapshot_preview: SnapshotPreviewController::disabled(),
        };
        state.refresh_snapshot_preview_for_selection_change(None);
        state
    }

    pub(crate) fn new_with_snapshot_loader(
        model: NavigationModel,
        loader: Box<dyn SnapshotLoader>,
    ) -> Self {
        let mut state = Self::new(model);
        state.snapshot_preview = SnapshotPreviewController::with_loader(loader);
        state.refresh_snapshot_preview_for_selection_change(None);
        state
    }

    #[cfg(test)]
    pub(crate) fn new_with_snapshot_loader_for_tests(
        model: NavigationModel,
        loader: Box<dyn SnapshotLoader>,
    ) -> Self {
        let mut state = Self::new(model);
        state.snapshot_preview = SnapshotPreviewController::with_loader_for_tests(loader);
        state.refresh_snapshot_preview_for_selection_change(None);
        state
    }

    pub(crate) fn mode(&self) -> InputMode {
        self.mode
    }

    pub(crate) fn filter_query(&self) -> &str {
        &self.filter_query
    }

    pub(crate) fn pending_action(&self) -> Option<&SelectionAction> {
        self.pending_action.as_ref()
    }

    pub(crate) fn visible_items(&self) -> Vec<VisibleItem> {
        if self.filter_query.is_empty() {
            self.model.flatten_visible()
        } else {
            self.model.flatten_filtered(&self.filter_query)
        }
    }

    pub(crate) fn selected_index(&self, visible_count: usize) -> Option<usize> {
        (visible_count > 0).then_some(self.selected_index.min(visible_count.saturating_sub(1)))
    }

    pub(crate) fn current_preview_target(&self) -> PreviewTarget {
        let visible_items = self.visible_items();
        let Some(selected_index) = self.selected_index(visible_items.len()) else {
            return if visible_items.is_empty() {
                PreviewTarget::Unavailable {
                    reason: NO_FILTER_RESULTS_REASON,
                }
            } else {
                PreviewTarget::Unavailable {
                    reason: NO_SELECTION_REASON,
                }
            };
        };

        self.model
            .resolve_preview_target(&visible_items[selected_index].node_id)
    }

    pub(crate) fn current_snapshot_preview_state(&self) -> &SnapshotPreviewState {
        self.snapshot_preview.state()
    }

    pub(crate) fn poll_preview_updates(&mut self) -> bool {
        self.snapshot_preview.poll_live_updates()
    }

    pub(crate) fn handle_key_event(&mut self, key: KeyEvent) -> EventResult {
        let previous_selection = self.selected_node_id();

        let result = if key.kind == KeyEventKind::Release {
            EventResult::Continue
        } else if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            EventResult::Quit
        } else {
            match key.code {
                KeyCode::Char('q') if self.mode == InputMode::Navigate => EventResult::Quit,
                KeyCode::Down => {
                    self.move_next();
                    EventResult::Continue
                }
                KeyCode::Up => {
                    self.move_previous();
                    EventResult::Continue
                }
                KeyCode::Char('j') if self.mode == InputMode::Navigate => {
                    self.move_next();
                    EventResult::Continue
                }
                KeyCode::Char('k') if self.mode == InputMode::Navigate => {
                    self.move_previous();
                    EventResult::Continue
                }
                KeyCode::Char('/') if self.mode == InputMode::Navigate => {
                    self.mode = InputMode::Filter;
                    EventResult::Continue
                }
                KeyCode::Esc => self.handle_escape(),
                KeyCode::Enter => {
                    self.stage_selection_action();
                    EventResult::Continue
                }
                KeyCode::Backspace if self.mode == InputMode::Filter => {
                    self.filter_query.pop();
                    self.on_filter_updated();
                    EventResult::Continue
                }
                KeyCode::Char(character)
                    if self.mode == InputMode::Filter
                        && !key
                            .modifiers
                            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    self.filter_query.push(character);
                    self.on_filter_updated();
                    EventResult::Continue
                }
                _ => EventResult::Continue,
            }
        };

        if result == EventResult::Quit {
            self.snapshot_preview.shutdown();
            return result;
        }

        self.refresh_snapshot_preview_for_selection_change(previous_selection);
        result
    }

    fn handle_escape(&mut self) -> EventResult {
        if !self.filter_query.is_empty() {
            self.filter_query.clear();
            self.clamp_selection();
            return EventResult::Continue;
        }

        if self.mode == InputMode::Filter {
            self.mode = InputMode::Navigate;
            return EventResult::Continue;
        }

        EventResult::Quit
    }

    fn move_next(&mut self) {
        let visible_count = self.visible_items().len();
        if visible_count > 0 {
            self.selected_index = (self.selected_index + 1).min(visible_count - 1);
        }
    }

    fn move_previous(&mut self) {
        self.selected_index = self.selected_index.saturating_sub(1);
    }

    fn stage_selection_action(&mut self) {
        let visible_items = self.visible_items();
        let Some(selected_index) = self.selected_index(visible_items.len()) else {
            self.pending_action = Some(SelectionAction::NoAction);
            return;
        };

        self.pending_action = Some(
            self.model
                .resolve_selection_action(&visible_items[selected_index].node_id),
        );
    }

    fn on_filter_updated(&mut self) {
        self.selected_index = 0;
        self.mode = InputMode::Filter;
        self.clamp_selection();
    }

    fn clamp_selection(&mut self) {
        let visible_count = self.visible_items().len();
        if visible_count == 0 {
            self.selected_index = 0;
            return;
        }

        self.selected_index = self.selected_index.min(visible_count - 1);
    }

    fn selected_node_id(&self) -> Option<NodeId> {
        let visible_items = self.visible_items();
        self.selected_index(visible_items.len())
            .map(|selected_index| visible_items[selected_index].node_id.clone())
    }

    fn refresh_snapshot_preview_for_selection_change(
        &mut self,
        previous_selection: Option<NodeId>,
    ) {
        let current_selection = self.selected_node_id();
        if current_selection == previous_selection {
            return;
        }

        let preview_target = self.current_preview_target();
        self.snapshot_preview.refresh_for_target(&preview_target);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;
    use std::time::Duration;

    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use crate::navigation_model::SelectionAction;
    use crate::runtime_discovery::{PaneSnapshot, RuntimeDiscoveryError};
    use crate::tui::demo_data::load_navigation_model;
    use crate::tui::preview::{PaneSnapshotRequest, SnapshotLoader, SnapshotPreviewState};

    use super::{AppState, EventResult, InputMode};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn build_fixture_state() -> AppState {
        let model = load_navigation_model().expect("fixture model should load");
        AppState::new(model)
    }

    fn build_fixture_state_with_loader_for_tests(loader: Box<dyn SnapshotLoader>) -> AppState {
        let model = load_navigation_model().expect("fixture model should load");
        AppState::new_with_snapshot_loader_for_tests(model, loader)
    }

    struct ReadySnapshotLoader;

    impl SnapshotLoader for ReadySnapshotLoader {
        fn load_snapshot(
            &self,
            request: &PaneSnapshotRequest,
        ) -> Result<PaneSnapshot, RuntimeDiscoveryError> {
            Ok(PaneSnapshot::Ready(format!(
                "snapshot for {}:{}",
                request.session_name, request.pane_id
            )))
        }
    }

    #[derive(Default)]
    struct IncrementingSnapshotLoader {
        calls: AtomicUsize,
    }

    impl IncrementingSnapshotLoader {
        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    impl SnapshotLoader for IncrementingSnapshotLoader {
        fn load_snapshot(
            &self,
            _request: &PaneSnapshotRequest,
        ) -> Result<PaneSnapshot, RuntimeDiscoveryError> {
            let sequence = self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(PaneSnapshot::Ready(format!("frame-{sequence}")))
        }

        fn subscribe_live_snapshots_with_cancel(
            &self,
            _request: &PaneSnapshotRequest,
            on_snapshot: &mut dyn FnMut(PaneSnapshot),
            _is_cancelled: &dyn Fn() -> bool,
        ) -> Result<(), RuntimeDiscoveryError> {
            let sequence = self.calls.fetch_add(1, Ordering::SeqCst);
            on_snapshot(PaneSnapshot::Ready(format!("frame-{sequence}")));
            Ok(())
        }
    }

    #[test]
    fn navigation_clamps_to_list_bounds() {
        let mut state = build_fixture_state();
        let total_rows = state.visible_items().len();

        for _ in 0..(total_rows + 10) {
            assert_eq!(
                state.handle_key_event(key(KeyCode::Down)),
                EventResult::Continue
            );
        }
        assert_eq!(
            state.selected_index(state.visible_items().len()),
            Some(total_rows - 1)
        );

        for _ in 0..(total_rows + 10) {
            assert_eq!(
                state.handle_key_event(key(KeyCode::Up)),
                EventResult::Continue
            );
        }
        assert_eq!(state.selected_index(state.visible_items().len()), Some(0));
    }

    #[test]
    fn escape_in_filter_mode_requires_clear_then_exit_then_quit() {
        let mut state = build_fixture_state();

        assert_eq!(state.mode(), InputMode::Navigate);
        assert_eq!(
            state.handle_key_event(key(KeyCode::Char('/'))),
            EventResult::Continue
        );
        assert_eq!(state.mode(), InputMode::Filter);

        assert_eq!(
            state.handle_key_event(key(KeyCode::Char('l'))),
            EventResult::Continue
        );
        assert_eq!(
            state.handle_key_event(key(KeyCode::Char('o'))),
            EventResult::Continue
        );
        assert_eq!(state.filter_query(), "lo");

        assert_eq!(
            state.handle_key_event(key(KeyCode::Esc)),
            EventResult::Continue
        );
        assert_eq!(state.mode(), InputMode::Filter);
        assert_eq!(state.filter_query(), "");

        assert_eq!(
            state.handle_key_event(key(KeyCode::Esc)),
            EventResult::Continue
        );
        assert_eq!(state.mode(), InputMode::Navigate);

        assert_eq!(state.handle_key_event(key(KeyCode::Esc)), EventResult::Quit);
    }

    #[test]
    fn q_is_printable_in_filter_mode_and_does_not_quit() {
        let mut state = build_fixture_state();

        assert_eq!(state.mode(), InputMode::Navigate);
        assert_eq!(
            state.handle_key_event(key(KeyCode::Char('/'))),
            EventResult::Continue
        );
        assert_eq!(state.mode(), InputMode::Filter);

        assert_eq!(
            state.handle_key_event(key(KeyCode::Char('q'))),
            EventResult::Continue
        );
        assert_eq!(state.filter_query(), "q");
        assert_eq!(state.mode(), InputMode::Filter);
    }

    #[test]
    fn enter_stages_selection_action_without_execution() {
        let mut state = build_fixture_state();

        assert_eq!(
            state.handle_key_event(key(KeyCode::Enter)),
            EventResult::Continue
        );
        assert_eq!(
            state.pending_action(),
            Some(&SelectionAction::SwitchSession {
                session_name: "zellijess".to_owned(),
            })
        );

        assert_eq!(
            state.handle_key_event(key(KeyCode::Down)),
            EventResult::Continue
        );
        assert_eq!(
            state.handle_key_event(key(KeyCode::Down)),
            EventResult::Continue
        );
        assert_eq!(
            state.handle_key_event(key(KeyCode::Enter)),
            EventResult::Continue
        );
        assert_eq!(
            state.pending_action(),
            Some(&SelectionAction::FocusPane {
                session_name: "zellijess".to_owned(),
                tab_id: 1,
                tab_position: 0,
                pane_id: 3,
            })
        );

        assert_eq!(
            state.handle_key_event(key(KeyCode::Char('/'))),
            EventResult::Continue
        );
        assert_eq!(
            state.handle_key_event(key(KeyCode::Char('z'))),
            EventResult::Continue
        );
        assert_eq!(
            state.handle_key_event(key(KeyCode::Char('z'))),
            EventResult::Continue
        );
        assert_eq!(
            state.handle_key_event(key(KeyCode::Char('z'))),
            EventResult::Continue
        );
        assert!(state.visible_items().is_empty());

        assert_eq!(
            state.handle_key_event(key(KeyCode::Enter)),
            EventResult::Continue
        );
        assert_eq!(state.pending_action(), Some(&SelectionAction::NoAction));
    }

    #[test]
    fn pane_selection_uses_configured_snapshot_loader() {
        let model = load_navigation_model().expect("fixture model should load");
        let mut state = AppState::new_with_snapshot_loader(model, Box::new(ReadySnapshotLoader));

        assert_eq!(
            state.handle_key_event(key(KeyCode::Down)),
            EventResult::Continue
        );
        assert_eq!(
            state.handle_key_event(key(KeyCode::Down)),
            EventResult::Continue
        );

        assert!(matches!(
            state.current_snapshot_preview_state(),
            SnapshotPreviewState::Ready { body, .. } if body == "snapshot for zellijess:3"
        ));
    }

    #[test]
    fn poll_preview_updates_ingests_background_frames_for_selected_pane() {
        let loader = Arc::new(IncrementingSnapshotLoader::default());
        let mut state = build_fixture_state_with_loader_for_tests(Box::new(loader.clone()));

        assert_eq!(
            state.handle_key_event(key(KeyCode::Down)),
            EventResult::Continue
        );
        assert_eq!(
            state.handle_key_event(key(KeyCode::Down)),
            EventResult::Continue
        );

        assert!(matches!(
            state.current_snapshot_preview_state(),
            SnapshotPreviewState::Ready { body, .. } if body == "frame-0"
        ));

        let mut changed = false;
        for _ in 0..8 {
            thread::sleep(Duration::from_millis(10));
            if state.poll_preview_updates() {
                changed = true;
                break;
            }
        }

        assert!(changed);
        assert!(matches!(
            state.current_snapshot_preview_state(),
            SnapshotPreviewState::Ready { body, .. } if body != "frame-0"
        ));
        assert!(loader.calls() > 1);
    }
}
