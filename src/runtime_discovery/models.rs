use std::collections::BTreeMap;

use serde::de::Error as DeError;
use serde::{Deserialize, Deserializer};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SessionInfo {
    pub(crate) name: String,
    pub(crate) created_age: Option<String>,
    pub(crate) state: SessionState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum SessionState {
    Active,
    Current,
    Exited,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(crate) struct TabInfo {
    pub(crate) position: u64,
    pub(crate) name: String,
    pub(crate) active: bool,
    #[serde(default, deserialize_with = "deserialize_panes_to_hide")]
    pub(crate) panes_to_hide: Vec<u64>,
    #[serde(default)]
    pub(crate) is_fullscreen_active: bool,
    #[serde(default)]
    pub(crate) is_sync_panes_active: bool,
    #[serde(default)]
    pub(crate) are_floating_panes_visible: bool,
    #[serde(default)]
    pub(crate) other_focused_clients: Vec<u64>,
    #[serde(default)]
    pub(crate) active_swap_layout_name: Option<String>,
    #[serde(default)]
    pub(crate) is_swap_layout_dirty: bool,
    #[serde(default)]
    pub(crate) viewport_rows: Option<u64>,
    #[serde(default)]
    pub(crate) viewport_columns: Option<u64>,
    #[serde(default)]
    pub(crate) display_area_rows: Option<u64>,
    #[serde(default)]
    pub(crate) display_area_columns: Option<u64>,
    #[serde(default)]
    pub(crate) selectable_tiled_panes_count: Option<u64>,
    #[serde(default)]
    pub(crate) selectable_floating_panes_count: Option<u64>,
    pub(crate) tab_id: u64,
    #[serde(default)]
    pub(crate) has_bell_notification: bool,
    #[serde(default)]
    pub(crate) is_flashing_bell: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub(crate) struct PaneInfo {
    pub(crate) id: u64,
    #[serde(default)]
    pub(crate) is_plugin: bool,
    #[serde(default)]
    pub(crate) is_focused: bool,
    #[serde(default)]
    pub(crate) is_fullscreen: bool,
    #[serde(default)]
    pub(crate) is_floating: bool,
    #[serde(default)]
    pub(crate) is_suppressed: bool,
    #[serde(default)]
    pub(crate) title: Option<String>,
    #[serde(default)]
    pub(crate) exited: bool,
    #[serde(default)]
    pub(crate) exit_status: Option<i32>,
    #[serde(default)]
    pub(crate) is_held: bool,
    #[serde(default)]
    pub(crate) pane_x: Option<i64>,
    #[serde(default)]
    pub(crate) pane_content_x: Option<i64>,
    #[serde(default)]
    pub(crate) pane_y: Option<i64>,
    #[serde(default)]
    pub(crate) pane_content_y: Option<i64>,
    #[serde(default)]
    pub(crate) pane_rows: Option<i64>,
    #[serde(default)]
    pub(crate) pane_content_rows: Option<i64>,
    #[serde(default)]
    pub(crate) pane_columns: Option<i64>,
    #[serde(default)]
    pub(crate) pane_content_columns: Option<i64>,
    #[serde(default)]
    pub(crate) cursor_coordinates_in_pane: Option<Value>,
    #[serde(default)]
    pub(crate) terminal_command: Option<String>,
    #[serde(default)]
    pub(crate) plugin_url: Option<String>,
    #[serde(default)]
    pub(crate) is_selectable: bool,
    #[serde(default, deserialize_with = "deserialize_index_in_pane_group")]
    pub(crate) index_in_pane_group: Option<u64>,
    #[serde(default)]
    pub(crate) default_fg: Option<Value>,
    #[serde(default)]
    pub(crate) default_bg: Option<Value>,
    pub(crate) tab_id: u64,
    pub(crate) tab_position: u64,
    pub(crate) tab_name: String,
    #[serde(default)]
    pub(crate) pane_command: Option<String>,
    #[serde(default)]
    pub(crate) pane_cwd: Option<String>,
}

fn deserialize_panes_to_hide<'de, D>(deserializer: D) -> Result<Vec<u64>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum PanesToHideRepr {
        PaneIds(Vec<u64>),
        LegacyScalar(u64),
    }

    let repr = Option::<PanesToHideRepr>::deserialize(deserializer)?;
    match repr {
        None => Ok(Vec::new()),
        Some(PanesToHideRepr::PaneIds(pane_ids)) => Ok(pane_ids),
        Some(PanesToHideRepr::LegacyScalar(0)) => Ok(Vec::new()),
        Some(PanesToHideRepr::LegacyScalar(value)) => Err(D::Error::custom(format!(
            "unsupported non-zero numeric panes_to_hide shape: {value}"
        ))),
    }
}

fn deserialize_index_in_pane_group<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum PaneGroupIndexRepr {
        Numeric(u64),
        Map(BTreeMap<String, Value>),
        Null,
    }

    let repr = Option::<PaneGroupIndexRepr>::deserialize(deserializer)?;
    match repr {
        None | Some(PaneGroupIndexRepr::Null) => Ok(None),
        Some(PaneGroupIndexRepr::Numeric(index)) => Ok(Some(index)),
        Some(PaneGroupIndexRepr::Map(map)) if map.is_empty() => Ok(None),
        Some(PaneGroupIndexRepr::Map(_)) => Err(D::Error::custom(
            "index_in_pane_group object form must be empty",
        )),
    }
}
