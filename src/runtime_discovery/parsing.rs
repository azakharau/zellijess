use std::fmt;

use serde::de::DeserializeOwned;

use crate::runtime_discovery::models::{PaneInfo, SessionInfo, SessionState, TabInfo};

const CREATED_MARKER: &str = " [Created ";

#[derive(Debug)]
pub(crate) enum ParseError {
    InvalidSessionLine {
        line: String,
    },
    InvalidJson {
        context: &'static str,
        source: serde_json::Error,
    },
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSessionLine { line } => {
                write!(f, "unexpected session line format: `{line}`")
            }
            Self::InvalidJson { context, source } => {
                write!(f, "invalid json for {context}: {source}")
            }
        }
    }
}

impl std::error::Error for ParseError {}

pub(crate) fn parse_sessions_output(raw: &str) -> Result<Vec<SessionInfo>, ParseError> {
    raw.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(parse_session_line)
        .collect()
}

pub(crate) fn parse_tabs_output(raw: &str) -> Result<Vec<TabInfo>, ParseError> {
    parse_json_items(raw, "list-tabs")
}

pub(crate) fn parse_panes_output(raw: &str) -> Result<Vec<PaneInfo>, ParseError> {
    parse_json_items(raw, "list-panes")
}

fn parse_session_line(line: &str) -> Result<SessionInfo, ParseError> {
    if let Some((name, rest)) = line.split_once(CREATED_MARKER) {
        let Some((created_age, suffix)) = rest.split_once(']') else {
            return Err(ParseError::InvalidSessionLine {
                line: line.to_owned(),
            });
        };

        return Ok(SessionInfo {
            name: name.trim().to_owned(),
            created_age: Some(created_age.to_owned()),
            state: parse_session_state(suffix),
        });
    }

    Ok(SessionInfo {
        name: line.to_owned(),
        created_age: None,
        state: SessionState::Unknown,
    })
}

fn parse_session_state(suffix: &str) -> SessionState {
    let normalized_suffix = suffix.trim().to_ascii_lowercase();

    if normalized_suffix.is_empty() || normalized_suffix == "(active)" {
        SessionState::Active
    } else if normalized_suffix == "(current)" {
        SessionState::Current
    } else if normalized_suffix == "(exited - attach to resurrect)" {
        SessionState::Exited
    } else {
        SessionState::Unknown
    }
}

fn parse_json_items<T>(raw: &str, context: &'static str) -> Result<Vec<T>, ParseError>
where
    T: DeserializeOwned,
{
    if let Ok(items) = serde_json::from_str::<Vec<T>>(raw) {
        return Ok(items);
    }

    let lines: Vec<&str> = raw
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();

    if lines.is_empty() {
        return Ok(Vec::new());
    }

    let mut parsed_items = Vec::with_capacity(lines.len());
    for line in lines {
        let item = serde_json::from_str::<T>(line)
            .map_err(|source| ParseError::InvalidJson { context, source })?;
        parsed_items.push(item);
    }

    Ok(parsed_items)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use crate::runtime_discovery::models::SessionState;
    use crate::runtime_discovery::parsing::{
        parse_panes_output, parse_sessions_output, parse_tabs_output,
    };

    fn read_fixture(filename: &str) -> String {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join(filename);

        fs::read_to_string(path).expect("fixture should be readable")
    }

    #[test]
    fn parses_sessions_fixture() {
        let fixture = read_fixture("list-sessions.txt");
        let sessions = parse_sessions_output(&fixture).expect("sessions fixture should parse");

        assert_eq!(sessions.len(), 7);
        assert_eq!(sessions[0].name, "zellijess");
        assert_eq!(sessions[0].created_age.as_deref(), Some("4m 26s ago"));
        assert_eq!(sessions[0].state, SessionState::Active);
        assert_eq!(sessions[1].name, "work-base");
        assert_eq!(sessions[1].created_age.as_deref(), Some("1h 02m ago"));
        assert_eq!(sessions[1].state, SessionState::Active);
        assert_eq!(sessions[2].state, SessionState::Exited);
        assert_eq!(sessions[3].state, SessionState::Current);
        assert_eq!(sessions[4].state, SessionState::Active);
        assert_eq!(sessions[5].state, SessionState::Unknown);
        assert_eq!(sessions[6].name, "work-no-metadata");
        assert!(sessions[6].created_age.is_none());
        assert_eq!(sessions[6].state, SessionState::Unknown);
    }

    #[test]
    fn parses_tabs_ndjson_fallback_output() {
        let raw = r#"
{"position":1,"name":"editor","active":true,"tab_id":1}
{"position":2,"name":"logs","active":false,"tab_id":2}
"#;
        let tabs = parse_tabs_output(raw).expect("tabs ndjson should parse");

        assert_eq!(tabs.len(), 2);
        assert_eq!(tabs[0].name, "editor");
        assert_eq!(tabs[0].tab_id, 1);
        assert_eq!(tabs[1].name, "logs");
        assert_eq!(tabs[1].tab_id, 2);
    }

    #[test]
    fn parses_panes_ndjson_fallback_output() {
        let raw = r#"
{"id":3,"tab_id":1,"tab_position":1,"tab_name":"editor"}
{"id":4,"tab_id":1,"tab_position":1,"tab_name":"editor"}
"#;
        let panes = parse_panes_output(raw).expect("panes ndjson should parse");

        assert_eq!(panes.len(), 2);
        assert_eq!(panes[0].id, 3);
        assert_eq!(panes[0].tab_name, "editor");
        assert_eq!(panes[1].id, 4);
        assert_eq!(panes[1].tab_name, "editor");
    }

    #[test]
    fn parses_tabs_fixture() {
        let fixture = read_fixture("list-tabs.json");
        let tabs = parse_tabs_output(&fixture).expect("tabs fixture should parse");

        assert_eq!(tabs.len(), 2);
        assert_eq!(tabs[0].tab_id, 1);
        assert_eq!(tabs[0].name, "editor");
        assert!(tabs[0].active);
        assert!(tabs[0].panes_to_hide.is_empty());
        assert_eq!(tabs[1].name, "logs");
        assert!(tabs[1].panes_to_hide.is_empty());
    }

    #[test]
    fn parses_panes_fixture() {
        let fixture = read_fixture("list-panes.json");
        let panes = parse_panes_output(&fixture).expect("panes fixture should parse");

        assert_eq!(panes.len(), 2);
        assert_eq!(panes[0].id, 3);
        assert_eq!(
            panes[0].pane_cwd.as_deref(),
            Some("/workspace/sanitized/project")
        );
        assert!(panes[0].index_in_pane_group.is_none());
        assert_eq!(panes[1].id, 4);
        assert!(panes[1].pane_cwd.is_none());
        assert_eq!(panes[1].index_in_pane_group, Some(1));
    }
}
