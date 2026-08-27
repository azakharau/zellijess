use std::fmt;

use crate::navigation_model::{NavigationModel, SessionScopedData, SourceFreshness};
use crate::runtime_discovery::{RuntimeDiscoveryError, parse_runtime_snapshot};

const SESSIONS_FIXTURE: &str = include_str!("../../tests/fixtures/list-sessions.txt");
const TABS_FIXTURE: &str = include_str!("../../tests/fixtures/list-tabs.json");
const PANES_FIXTURE: &str = include_str!("../../tests/fixtures/list-panes.json");

#[derive(Debug)]
pub(crate) struct DemoDataError {
    source: RuntimeDiscoveryError,
}

impl fmt::Display for DemoDataError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "failed to parse fixture data: {}", self.source)
    }
}

impl std::error::Error for DemoDataError {}

pub(crate) fn load_navigation_model() -> Result<NavigationModel, DemoDataError> {
    let parsed = parse_runtime_snapshot(SESSIONS_FIXTURE, TABS_FIXTURE, PANES_FIXTURE)
        .map_err(|source| DemoDataError { source })?;

    let primary_session_name = parsed.sessions.first().map(|session| session.name.clone());
    let mut fixture_tabs = Some(parsed.tabs);
    let mut fixture_panes = Some(parsed.panes);

    let scoped_data = parsed
        .sessions
        .into_iter()
        .map(|session| {
            let is_primary_session = primary_session_name.as_deref() == Some(session.name.as_str());
            SessionScopedData {
                session,
                tabs: if is_primary_session {
                    fixture_tabs.take().unwrap_or_default()
                } else {
                    Vec::new()
                },
                panes: if is_primary_session {
                    fixture_panes.take().unwrap_or_default()
                } else {
                    Vec::new()
                },
                source_freshness: SourceFreshness::Runtime,
            }
        })
        .collect();

    Ok(NavigationModel::from_session_scoped_data(scoped_data))
}
