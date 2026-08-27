use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};

use crate::runtime_discovery::{PaneInfo, SessionInfo, SessionState, TabInfo};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum SourceFreshness {
    #[default]
    Runtime,
    Subscription,
    Cache,
    Stale,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum PaneKind {
    Terminal,
    Plugin,
}

impl PaneKind {
    fn from_pane_info(pane: &PaneInfo) -> Self {
        if pane.is_plugin {
            Self::Plugin
        } else {
            Self::Terminal
        }
    }

    fn as_filter_token(self) -> &'static str {
        match self {
            Self::Terminal => "terminal",
            Self::Plugin => "plugin",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) enum NodeId {
    Session {
        session_name: String,
    },
    Tab {
        session_name: String,
        tab_id: u64,
        tab_position: u64,
    },
    Pane {
        session_name: String,
        tab_id: u64,
        tab_position: u64,
        pane_id: u64,
        pane_kind: PaneKind,
    },
}

impl PartialEq for NodeId {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (
                Self::Session {
                    session_name: left_session,
                },
                Self::Session {
                    session_name: right_session,
                },
            ) => left_session == right_session,
            (
                Self::Tab {
                    session_name: left_session,
                    tab_id: left_tab_id,
                    ..
                },
                Self::Tab {
                    session_name: right_session,
                    tab_id: right_tab_id,
                    ..
                },
            ) => left_session == right_session && left_tab_id == right_tab_id,
            (
                Self::Pane {
                    session_name: left_session,
                    tab_id: left_tab_id,
                    pane_id: left_pane_id,
                    pane_kind: left_pane_kind,
                    ..
                },
                Self::Pane {
                    session_name: right_session,
                    tab_id: right_tab_id,
                    pane_id: right_pane_id,
                    pane_kind: right_pane_kind,
                    ..
                },
            ) => {
                left_session == right_session
                    && left_tab_id == right_tab_id
                    && left_pane_id == right_pane_id
                    && left_pane_kind == right_pane_kind
            }
            _ => false,
        }
    }
}

impl Eq for NodeId {}

impl Hash for NodeId {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            Self::Session { session_name } => {
                0_u8.hash(state);
                session_name.hash(state);
            }
            Self::Tab {
                session_name,
                tab_id,
                ..
            } => {
                1_u8.hash(state);
                session_name.hash(state);
                tab_id.hash(state);
            }
            Self::Pane {
                session_name,
                tab_id,
                pane_id,
                pane_kind,
                ..
            } => {
                2_u8.hash(state);
                session_name.hash(state);
                tab_id.hash(state);
                pane_id.hash(state);
                pane_kind.hash(state);
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VisibleItem {
    pub(crate) depth: usize,
    pub(crate) label: String,
    pub(crate) node_id: NodeId,
    pub(crate) selectable: bool,
    pub(crate) filter_text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PreviewTarget {
    SessionSummary {
        session_name: String,
        source_freshness: SourceFreshness,
    },
    TabSummary {
        session_name: String,
        tab_id: u64,
        tab_position: u64,
        source_freshness: SourceFreshness,
    },
    PaneSnapshotCandidate {
        session_name: String,
        tab_id: u64,
        tab_position: u64,
        pane_id: u64,
        source_freshness: SourceFreshness,
    },
    Unavailable {
        reason: &'static str,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SelectionAction {
    SwitchSession {
        session_name: String,
    },
    SwitchTab {
        session_name: String,
        tab_id: u64,
        tab_position: u64,
    },
    FocusPane {
        session_name: String,
        tab_id: u64,
        tab_position: u64,
        pane_id: u64,
    },
    NoAction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PaneNode {
    pub(crate) node_id: NodeId,
    pub(crate) pane_id: u64,
    pub(crate) pane_kind: PaneKind,
    pub(crate) title: Option<String>,
    pub(crate) terminal_command: Option<String>,
    pub(crate) pane_command: Option<String>,
    pub(crate) pane_cwd: Option<String>,
    pub(crate) selectable: bool,
    pub(crate) source_freshness: SourceFreshness,
    pub(crate) filter_text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TabNode {
    pub(crate) node_id: NodeId,
    pub(crate) tab_id: u64,
    pub(crate) tab_position: u64,
    pub(crate) name: String,
    pub(crate) active: bool,
    pub(crate) panes: Vec<PaneNode>,
    pub(crate) source_freshness: SourceFreshness,
    pub(crate) filter_text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SessionNode {
    pub(crate) node_id: NodeId,
    pub(crate) name: String,
    pub(crate) created_age: Option<String>,
    pub(crate) state: SessionState,
    pub(crate) tabs: Vec<TabNode>,
    pub(crate) source_freshness: SourceFreshness,
    pub(crate) filter_text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NavigationModel {
    pub(crate) sessions: Vec<SessionNode>,
}

#[derive(Debug, Clone)]
pub(crate) struct SessionScopedData {
    pub(crate) session: SessionInfo,
    pub(crate) tabs: Vec<TabInfo>,
    pub(crate) panes: Vec<PaneInfo>,
    pub(crate) source_freshness: SourceFreshness,
}

impl NavigationModel {
    pub(crate) fn from_session_scoped_data(scoped_data: Vec<SessionScopedData>) -> Self {
        let sessions = scoped_data
            .into_iter()
            .map(SessionNode::from_session_scoped_data)
            .collect();

        Self { sessions }
    }

    pub(crate) fn flatten_visible(&self) -> Vec<VisibleItem> {
        self.flatten_filtered("")
    }

    pub(crate) fn flatten_filtered(&self, filter_query: &str) -> Vec<VisibleItem> {
        let needle = normalize_filter_query(filter_query);
        let has_filter = !needle.is_empty();
        let mut items = Vec::new();

        for session in &self.sessions {
            let session_matches = matches_filter(&session.filter_text, &needle);
            let mut session_descendants = Vec::new();

            for tab in &session.tabs {
                let tab_matches = matches_filter(&tab.filter_text, &needle);
                let mut tab_descendants: Vec<VisibleItem> = tab
                    .panes
                    .iter()
                    .filter(|pane| matches_filter(&pane.filter_text, &needle))
                    .map(|pane| pane.visible_item(2))
                    .collect();

                if !has_filter || tab_matches || !tab_descendants.is_empty() {
                    let mut tab_items = vec![tab.visible_item(1)];
                    tab_items.append(&mut tab_descendants);
                    session_descendants.extend(tab_items);
                }
            }

            if !has_filter || session_matches || !session_descendants.is_empty() {
                items.push(session.visible_item(0));
                items.extend(session_descendants);
            }
        }

        items
    }

    pub(crate) fn resolve_preview_target(&self, node_id: &NodeId) -> PreviewTarget {
        match node_id {
            NodeId::Session { session_name } => self
                .find_session(session_name)
                .map(|session| PreviewTarget::SessionSummary {
                    session_name: session.name.clone(),
                    source_freshness: session.source_freshness,
                })
                .unwrap_or(PreviewTarget::Unavailable {
                    reason: "selected node is missing",
                }),
            NodeId::Tab {
                session_name,
                tab_id,
                tab_position,
            } => self
                .find_tab(session_name, *tab_id, *tab_position)
                .map(|tab| PreviewTarget::TabSummary {
                    session_name: session_name.clone(),
                    tab_id: tab.tab_id,
                    tab_position: tab.tab_position,
                    source_freshness: tab.source_freshness,
                })
                .unwrap_or(PreviewTarget::Unavailable {
                    reason: "selected node is missing",
                }),
            NodeId::Pane {
                session_name,
                tab_id,
                tab_position,
                pane_id,
                pane_kind,
            } => {
                let Some((tab, pane)) =
                    self.find_pane(session_name, *tab_id, *tab_position, *pane_id, *pane_kind)
                else {
                    return PreviewTarget::Unavailable {
                        reason: "selected node is missing",
                    };
                };

                if pane.pane_kind == PaneKind::Terminal && pane.selectable {
                    PreviewTarget::PaneSnapshotCandidate {
                        session_name: session_name.clone(),
                        tab_id: *tab_id,
                        tab_position: tab.tab_position,
                        pane_id: *pane_id,
                        source_freshness: pane.source_freshness,
                    }
                } else {
                    PreviewTarget::Unavailable {
                        reason: "pane has no terminal preview target",
                    }
                }
            }
        }
    }

    pub(crate) fn resolve_selection_action(&self, node_id: &NodeId) -> SelectionAction {
        match node_id {
            NodeId::Session { session_name } => {
                if self.find_session(session_name).is_some() {
                    SelectionAction::SwitchSession {
                        session_name: session_name.clone(),
                    }
                } else {
                    SelectionAction::NoAction
                }
            }
            NodeId::Tab {
                session_name,
                tab_id,
                tab_position,
            } => self
                .find_tab(session_name, *tab_id, *tab_position)
                .map(|tab| SelectionAction::SwitchTab {
                    session_name: session_name.clone(),
                    tab_id: tab.tab_id,
                    tab_position: tab.tab_position,
                })
                .unwrap_or(SelectionAction::NoAction),
            NodeId::Pane {
                session_name,
                tab_id,
                tab_position,
                pane_id,
                pane_kind,
            } => self
                .find_pane(session_name, *tab_id, *tab_position, *pane_id, *pane_kind)
                .and_then(|(tab, pane)| {
                    (pane.pane_kind == PaneKind::Terminal && pane.selectable).then(|| {
                        SelectionAction::FocusPane {
                            session_name: session_name.clone(),
                            tab_id: *tab_id,
                            tab_position: tab.tab_position,
                            pane_id: *pane_id,
                        }
                    })
                })
                .unwrap_or(SelectionAction::NoAction),
        }
    }

    fn find_session(&self, session_name: &str) -> Option<&SessionNode> {
        self.sessions
            .iter()
            .find(|session| session.name == session_name)
    }

    fn find_tab(&self, session_name: &str, tab_id: u64, tab_position: u64) -> Option<&TabNode> {
        self.find_session(session_name).and_then(|session| {
            session
                .tabs
                .iter()
                .find(|tab| tab.tab_id == tab_id && tab.tab_position == tab_position)
                .or_else(|| session.tabs.iter().find(|tab| tab.tab_id == tab_id))
        })
    }

    fn find_pane(
        &self,
        session_name: &str,
        tab_id: u64,
        tab_position: u64,
        pane_id: u64,
        pane_kind: PaneKind,
    ) -> Option<(&TabNode, &PaneNode)> {
        self.find_tab(session_name, tab_id, tab_position)
            .and_then(|tab| {
                tab.panes
                    .iter()
                    .find(|pane| pane.pane_id == pane_id && pane.pane_kind == pane_kind)
                    .map(|pane| (tab, pane))
            })
    }
}

impl SessionNode {
    fn from_session_scoped_data(scoped: SessionScopedData) -> Self {
        let SessionScopedData {
            session,
            mut tabs,
            panes,
            source_freshness,
        } = scoped;

        tabs.sort_by_key(|tab| (tab.position, tab.tab_id));

        let mut panes_by_tab: BTreeMap<u64, Vec<PaneInfo>> = BTreeMap::new();
        for pane in panes {
            panes_by_tab.entry(pane.tab_id).or_default().push(pane);
        }

        let session_name = session.name.clone();
        let tabs = tabs
            .into_iter()
            .map(|tab| {
                let tab_panes = panes_by_tab.remove(&tab.tab_id).unwrap_or_default();
                TabNode::from_tab_info(&session_name, tab, tab_panes, source_freshness)
            })
            .collect();

        let state_token = session_state_filter_token(session.state);
        let created_age = session.created_age.clone().unwrap_or_default();
        let filter_text =
            format!("{} {} {}", session_name, state_token, created_age).to_ascii_lowercase();

        Self {
            node_id: NodeId::Session {
                session_name: session_name.clone(),
            },
            name: session_name,
            created_age: session.created_age,
            state: session.state,
            tabs,
            source_freshness,
            filter_text,
        }
    }

    fn visible_item(&self, depth: usize) -> VisibleItem {
        VisibleItem {
            depth,
            label: format!(
                "session {} ({})",
                self.name,
                session_state_filter_token(self.state)
            ),
            node_id: self.node_id.clone(),
            selectable: true,
            filter_text: self.filter_text.clone(),
        }
    }
}

impl TabNode {
    fn from_tab_info(
        session_name: &str,
        tab: TabInfo,
        panes: Vec<PaneInfo>,
        source_freshness: SourceFreshness,
    ) -> Self {
        let tab_position = tab.position;
        let tab_id = tab.tab_id;
        let tab_name = tab.name;
        let active = tab.active;

        let panes = panes
            .into_iter()
            .map(|pane| {
                PaneNode::from_pane_info(
                    session_name,
                    tab_id,
                    tab_position,
                    tab_name.as_str(),
                    pane,
                    source_freshness,
                )
            })
            .collect();

        let filter_text = format!("{} {} {}", tab_name, tab_position, tab_id).to_ascii_lowercase();

        Self {
            node_id: NodeId::Tab {
                session_name: session_name.to_owned(),
                tab_id,
                tab_position,
            },
            tab_id,
            tab_position,
            name: tab_name,
            active,
            panes,
            source_freshness,
            filter_text,
        }
    }

    fn visible_item(&self, depth: usize) -> VisibleItem {
        VisibleItem {
            depth,
            label: format!("tab {}: {}", self.tab_position, self.name),
            node_id: self.node_id.clone(),
            selectable: true,
            filter_text: self.filter_text.clone(),
        }
    }
}

impl PaneNode {
    fn from_pane_info(
        session_name: &str,
        tab_id: u64,
        tab_position: u64,
        tab_name: &str,
        pane: PaneInfo,
        source_freshness: SourceFreshness,
    ) -> Self {
        let pane_kind = PaneKind::from_pane_info(&pane);
        let selectable = pane.selectable_terminal_candidate(pane_kind);
        let pane_id = pane.id;
        let title = pane.title;
        let terminal_command = pane.terminal_command;
        let pane_command = pane.pane_command;
        let pane_cwd = pane.pane_cwd;
        let plugin_url = pane.plugin_url;
        let pane_kind_token = pane_kind.as_filter_token();
        let pane_id_token = pane_id.to_string();
        let filter_text = format!(
            "{} {} {} {} {} {} {} {} {}",
            tab_name,
            tab_position,
            tab_id,
            pane_id_token,
            pane_kind_token,
            title.as_deref().unwrap_or_default(),
            terminal_command.as_deref().unwrap_or_default(),
            pane_command.as_deref().unwrap_or_default(),
            pane_cwd
                .as_deref()
                .unwrap_or(plugin_url.as_deref().unwrap_or_default()),
        )
        .to_ascii_lowercase();

        Self {
            node_id: NodeId::Pane {
                session_name: session_name.to_owned(),
                tab_id,
                tab_position,
                pane_id,
                pane_kind,
            },
            pane_id,
            pane_kind,
            title,
            terminal_command,
            pane_command,
            pane_cwd,
            selectable,
            source_freshness,
            filter_text,
        }
    }

    fn visible_item(&self, depth: usize) -> VisibleItem {
        let pane_kind = self.pane_kind.as_filter_token();
        let title = self.title.as_deref().unwrap_or(pane_kind);

        VisibleItem {
            depth,
            label: format!("{pane_kind} pane {}: {title}", self.pane_id),
            node_id: self.node_id.clone(),
            selectable: self.selectable,
            filter_text: self.filter_text.clone(),
        }
    }
}

trait PaneInfoExt {
    fn selectable_terminal_candidate(&self, pane_kind: PaneKind) -> bool;
}

impl PaneInfoExt for PaneInfo {
    fn selectable_terminal_candidate(&self, pane_kind: PaneKind) -> bool {
        self.is_selectable && pane_kind == PaneKind::Terminal
    }
}

fn normalize_filter_query(filter_query: &str) -> String {
    filter_query.trim().to_ascii_lowercase()
}

fn matches_filter(filter_text: &str, needle: &str) -> bool {
    needle.is_empty() || filter_text.contains(needle)
}

fn session_state_filter_token(state: SessionState) -> &'static str {
    match state {
        SessionState::Active => "active",
        SessionState::Current => "current",
        SessionState::Exited => "exited",
        SessionState::Unknown => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::fs;
    use std::path::Path;

    use crate::runtime_discovery::{
        parse_panes_output_for_tests, parse_sessions_output_for_tests, parse_tabs_output_for_tests,
    };

    use super::{
        NavigationModel, NodeId, PaneKind, PreviewTarget, SelectionAction, SessionScopedData,
        SourceFreshness,
    };

    fn read_fixture(filename: &str) -> String {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join(filename);

        fs::read_to_string(path).expect("fixture should be readable")
    }

    fn build_fixture_model() -> NavigationModel {
        let sessions_raw = read_fixture("list-sessions.txt");
        let tabs_raw = read_fixture("list-tabs.json");
        let panes_raw = read_fixture("list-panes.json");

        let sessions = parse_sessions_output_for_tests(&sessions_raw)
            .expect("sessions fixture should parse for navigation model tests");
        let tabs =
            parse_tabs_output_for_tests(&tabs_raw).expect("tabs fixture should parse for tests");
        let mut panes =
            parse_panes_output_for_tests(&panes_raw).expect("panes fixture should parse for tests");

        let mut plugin_pane = panes[0].clone();
        plugin_pane.is_plugin = true;
        plugin_pane.is_selectable = false;
        plugin_pane.title = Some("status".to_owned());
        plugin_pane.terminal_command = None;
        plugin_pane.pane_command = None;
        plugin_pane.pane_cwd = None;
        plugin_pane.plugin_url = Some("zellij:status-bar".to_owned());
        panes.push(plugin_pane);

        let mut non_selectable_terminal = panes[1].clone();
        non_selectable_terminal.id = 9;
        non_selectable_terminal.is_selectable = false;
        non_selectable_terminal.title = Some("monitor".to_owned());
        panes.push(non_selectable_terminal);

        NavigationModel::from_session_scoped_data(vec![
            SessionScopedData {
                session: sessions[0].clone(),
                tabs: tabs.clone(),
                panes,
                source_freshness: SourceFreshness::Runtime,
            },
            SessionScopedData {
                session: sessions[3].clone(),
                tabs: Vec::new(),
                panes: Vec::new(),
                source_freshness: SourceFreshness::Cache,
            },
        ])
    }

    #[test]
    fn builds_tree_with_stable_session_tab_and_pane_ids() {
        let model = build_fixture_model();

        assert_eq!(model.sessions.len(), 2);
        assert!(matches!(
            &model.sessions[0].node_id,
            NodeId::Session { session_name } if session_name == "zellijess"
        ));

        let editor_tab = &model.sessions[0].tabs[0];
        assert!(matches!(
            &editor_tab.node_id,
            NodeId::Tab {
                session_name,
                tab_id: 1,
                tab_position: 0,
            } if session_name == "zellijess"
        ));

        assert!(editor_tab.panes.iter().any(|pane| {
            matches!(
                &pane.node_id,
                NodeId::Pane {
                    tab_id: 1,
                    tab_position: 0,
                    pane_id: 3,
                    pane_kind: PaneKind::Terminal,
                    ..
                }
            )
        }));
        assert!(editor_tab.panes.iter().any(|pane| {
            matches!(
                &pane.node_id,
                NodeId::Pane {
                    tab_id: 1,
                    tab_position: 0,
                    pane_id: 3,
                    pane_kind: PaneKind::Plugin,
                    ..
                }
            )
        }));
    }

    #[test]
    fn flatten_visible_returns_session_tab_pane_order() {
        let model = build_fixture_model();
        let visible = model.flatten_visible();

        let ids: Vec<NodeId> = visible.iter().map(|item| item.node_id.clone()).collect();
        assert_eq!(ids.len(), 8);
        assert!(matches!(&ids[0], NodeId::Session { session_name } if session_name == "zellijess"));
        assert!(matches!(
            &ids[1],
            NodeId::Tab {
                tab_id: 1,
                tab_position: 0,
                ..
            }
        ));
        assert!(matches!(
            &ids[2],
            NodeId::Pane {
                pane_id: 3,
                pane_kind: PaneKind::Terminal,
                ..
            }
        ));
        assert!(matches!(
            &ids[3],
            NodeId::Pane {
                pane_id: 4,
                pane_kind: PaneKind::Terminal,
                ..
            }
        ));
        assert!(matches!(
            &ids[4],
            NodeId::Pane {
                pane_id: 3,
                pane_kind: PaneKind::Plugin,
                ..
            }
        ));
        assert!(matches!(
            &ids[5],
            NodeId::Pane {
                pane_id: 9,
                pane_kind: PaneKind::Terminal,
                ..
            }
        ));
        assert!(matches!(
            &ids[6],
            NodeId::Tab {
                tab_id: 2,
                tab_position: 1,
                ..
            }
        ));
        assert!(
            matches!(&ids[7], NodeId::Session { session_name } if session_name == "work-current")
        );

        assert_eq!(visible[0].depth, 0);
        assert_eq!(visible[1].depth, 1);
        assert_eq!(visible[2].depth, 2);
    }

    #[test]
    fn filtering_matches_session_tab_and_pane_fields_case_insensitively() {
        let model = build_fixture_model();

        let session_filtered = model.flatten_filtered("CURRENT");
        assert!(session_filtered.iter().any(
            |item| matches!(&item.node_id, NodeId::Session { session_name } if session_name == "work-current")
        ));

        let tab_filtered = model.flatten_filtered("LoGs");
        assert!(
            tab_filtered
                .iter()
                .any(|item| matches!(&item.node_id, NodeId::Tab { tab_id: 2, .. }))
        );

        let pane_command_filtered = model.flatten_filtered("nvim");
        assert!(pane_command_filtered.iter().any(|item| {
            matches!(
                &item.node_id,
                NodeId::Pane {
                    pane_id: 3,
                    pane_kind: PaneKind::Terminal,
                    ..
                }
            )
        }));

        let pane_cwd_filtered = model.flatten_filtered("/workspace/sanitized/project");
        assert!(pane_cwd_filtered.iter().any(|item| {
            matches!(
                &item.node_id,
                NodeId::Pane {
                    pane_id: 3,
                    pane_kind: PaneKind::Terminal,
                    ..
                }
            )
        }));

        let pane_kind_filtered = model.flatten_filtered("plugin");
        assert!(pane_kind_filtered.iter().any(|item| {
            matches!(
                &item.node_id,
                NodeId::Pane {
                    pane_id: 3,
                    pane_kind: PaneKind::Plugin,
                    ..
                }
            )
        }));
    }

    #[test]
    fn resolves_preview_targets_and_selection_actions_without_fake_plugin_focus() {
        let model = build_fixture_model();

        let session_id = NodeId::Session {
            session_name: "zellijess".to_owned(),
        };
        assert_eq!(
            model.resolve_preview_target(&session_id),
            PreviewTarget::SessionSummary {
                session_name: "zellijess".to_owned(),
                source_freshness: SourceFreshness::Runtime,
            }
        );
        assert_eq!(
            model.resolve_selection_action(&session_id),
            SelectionAction::SwitchSession {
                session_name: "zellijess".to_owned(),
            }
        );

        let tab_id = NodeId::Tab {
            session_name: "zellijess".to_owned(),
            tab_id: 1,
            tab_position: 0,
        };
        assert_eq!(
            model.resolve_preview_target(&tab_id),
            PreviewTarget::TabSummary {
                session_name: "zellijess".to_owned(),
                tab_id: 1,
                tab_position: 0,
                source_freshness: SourceFreshness::Runtime,
            }
        );
        assert_eq!(
            model.resolve_selection_action(&tab_id),
            SelectionAction::SwitchTab {
                session_name: "zellijess".to_owned(),
                tab_id: 1,
                tab_position: 0,
            }
        );

        let terminal_pane_id = NodeId::Pane {
            session_name: "zellijess".to_owned(),
            tab_id: 1,
            tab_position: 0,
            pane_id: 3,
            pane_kind: PaneKind::Terminal,
        };
        assert_eq!(
            model.resolve_preview_target(&terminal_pane_id),
            PreviewTarget::PaneSnapshotCandidate {
                session_name: "zellijess".to_owned(),
                tab_id: 1,
                tab_position: 0,
                pane_id: 3,
                source_freshness: SourceFreshness::Runtime,
            }
        );
        assert_eq!(
            model.resolve_selection_action(&terminal_pane_id),
            SelectionAction::FocusPane {
                session_name: "zellijess".to_owned(),
                tab_id: 1,
                tab_position: 0,
                pane_id: 3,
            }
        );

        let plugin_pane_id = NodeId::Pane {
            session_name: "zellijess".to_owned(),
            tab_id: 1,
            tab_position: 0,
            pane_id: 3,
            pane_kind: PaneKind::Plugin,
        };
        assert!(matches!(
            model.resolve_preview_target(&plugin_pane_id),
            PreviewTarget::Unavailable { .. }
        ));
        assert_eq!(
            model.resolve_selection_action(&plugin_pane_id),
            SelectionAction::NoAction
        );

        let non_selectable_terminal_pane_id = NodeId::Pane {
            session_name: "zellijess".to_owned(),
            tab_id: 1,
            tab_position: 0,
            pane_id: 9,
            pane_kind: PaneKind::Terminal,
        };
        assert!(matches!(
            model.resolve_preview_target(&non_selectable_terminal_pane_id),
            PreviewTarget::Unavailable { .. }
        ));
        assert_eq!(
            model.resolve_selection_action(&non_selectable_terminal_pane_id),
            SelectionAction::NoAction
        );
    }

    #[test]
    fn source_freshness_is_carried_through_nodes_and_preview_resolution() {
        let model = build_fixture_model();

        assert_eq!(model.sessions[0].source_freshness, SourceFreshness::Runtime);
        assert_eq!(
            model.sessions[0].tabs[0].source_freshness,
            SourceFreshness::Runtime
        );
        assert_eq!(
            model.sessions[0].tabs[0].panes[0].source_freshness,
            SourceFreshness::Runtime
        );
        assert_eq!(model.sessions[1].source_freshness, SourceFreshness::Cache);

        let cached_session_preview = model.resolve_preview_target(&NodeId::Session {
            session_name: "work-current".to_owned(),
        });
        assert_eq!(
            cached_session_preview,
            PreviewTarget::SessionSummary {
                session_name: "work-current".to_owned(),
                source_freshness: SourceFreshness::Cache,
            }
        );
    }

    #[test]
    fn resolves_tab_and_pane_from_prior_node_id_after_tab_reorder_refresh() {
        let sessions_raw = read_fixture("list-sessions.txt");
        let tabs_raw = read_fixture("list-tabs.json");
        let panes_raw = read_fixture("list-panes.json");

        let sessions = parse_sessions_output_for_tests(&sessions_raw)
            .expect("sessions fixture should parse for navigation model tests");
        let tabs =
            parse_tabs_output_for_tests(&tabs_raw).expect("tabs fixture should parse for tests");
        let panes =
            parse_panes_output_for_tests(&panes_raw).expect("panes fixture should parse for tests");

        let initial_model = NavigationModel::from_session_scoped_data(vec![SessionScopedData {
            session: sessions[0].clone(),
            tabs: tabs.clone(),
            panes: panes.clone(),
            source_freshness: SourceFreshness::Runtime,
        }]);

        let stale_tab_id = initial_model.sessions[0].tabs[0].node_id.clone();
        let stale_terminal_pane_id = initial_model.sessions[0].tabs[0]
            .panes
            .iter()
            .find(|pane| pane.pane_id == 3 && pane.pane_kind == PaneKind::Terminal)
            .expect("fixture should have a terminal pane in tab 1")
            .node_id
            .clone();

        let mut reordered_tabs = tabs;
        for tab in &mut reordered_tabs {
            match tab.tab_id {
                1 => tab.position = 1,
                2 => tab.position = 0,
                _ => {}
            }
        }

        let mut reordered_panes = panes;
        for pane in &mut reordered_panes {
            match pane.tab_id {
                1 => pane.tab_position = 1,
                2 => pane.tab_position = 0,
                _ => {}
            }
        }

        let refreshed_model = NavigationModel::from_session_scoped_data(vec![SessionScopedData {
            session: sessions[0].clone(),
            tabs: reordered_tabs,
            panes: reordered_panes,
            source_freshness: SourceFreshness::Runtime,
        }]);

        let refreshed_tab_id = refreshed_model.sessions[0]
            .tabs
            .iter()
            .find(|tab| tab.tab_id == 1)
            .expect("refreshed model should include tab 1")
            .node_id
            .clone();
        let refreshed_terminal_pane_id = refreshed_model.sessions[0]
            .tabs
            .iter()
            .find(|tab| tab.tab_id == 1)
            .expect("refreshed model should include tab 1")
            .panes
            .iter()
            .find(|pane| pane.pane_id == 3 && pane.pane_kind == PaneKind::Terminal)
            .expect("refreshed model should include pane 3 in tab 1")
            .node_id
            .clone();

        assert_eq!(stale_tab_id, refreshed_tab_id);
        assert_eq!(stale_terminal_pane_id, refreshed_terminal_pane_id);

        let mut stable_ids = HashSet::new();
        stable_ids.insert(stale_tab_id.clone());
        stable_ids.insert(stale_terminal_pane_id.clone());
        assert!(stable_ids.contains(&refreshed_tab_id));
        assert!(stable_ids.contains(&refreshed_terminal_pane_id));

        assert_eq!(
            refreshed_model.resolve_preview_target(&stale_tab_id),
            PreviewTarget::TabSummary {
                session_name: "zellijess".to_owned(),
                tab_id: 1,
                tab_position: 1,
                source_freshness: SourceFreshness::Runtime,
            }
        );
        assert_eq!(
            refreshed_model.resolve_selection_action(&stale_tab_id),
            SelectionAction::SwitchTab {
                session_name: "zellijess".to_owned(),
                tab_id: 1,
                tab_position: 1,
            }
        );

        assert_eq!(
            refreshed_model.resolve_preview_target(&stale_terminal_pane_id),
            PreviewTarget::PaneSnapshotCandidate {
                session_name: "zellijess".to_owned(),
                tab_id: 1,
                tab_position: 1,
                pane_id: 3,
                source_freshness: SourceFreshness::Runtime,
            }
        );
        assert_eq!(
            refreshed_model.resolve_selection_action(&stale_terminal_pane_id),
            SelectionAction::FocusPane {
                session_name: "zellijess".to_owned(),
                tab_id: 1,
                tab_position: 1,
                pane_id: 3,
            }
        );
    }

    #[test]
    fn attaches_panes_to_tab_by_tab_id_when_pane_tab_position_is_stale() {
        let sessions_raw = read_fixture("list-sessions.txt");
        let tabs_raw = read_fixture("list-tabs.json");
        let panes_raw = read_fixture("list-panes.json");

        let sessions = parse_sessions_output_for_tests(&sessions_raw)
            .expect("sessions fixture should parse for navigation model tests");
        let tabs =
            parse_tabs_output_for_tests(&tabs_raw).expect("tabs fixture should parse for tests");
        let mut panes =
            parse_panes_output_for_tests(&panes_raw).expect("panes fixture should parse for tests");

        for pane in &mut panes {
            if pane.tab_id == 1 {
                pane.tab_position = 777;
            }
        }

        let model = NavigationModel::from_session_scoped_data(vec![SessionScopedData {
            session: sessions[0].clone(),
            tabs,
            panes,
            source_freshness: SourceFreshness::Runtime,
        }]);

        let tab_one = model.sessions[0]
            .tabs
            .iter()
            .find(|tab| tab.tab_id == 1)
            .expect("model should include tab 1");
        assert!(
            tab_one
                .panes
                .iter()
                .any(|pane| pane.pane_id == 3 && pane.pane_kind == PaneKind::Terminal)
        );
        assert!(
            tab_one
                .panes
                .iter()
                .any(|pane| pane.pane_id == 4 && pane.pane_kind == PaneKind::Terminal)
        );
    }
}
