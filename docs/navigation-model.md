# Navigation Model (M2 Core)

## Scope

M2 adds a normalized in-memory model:

- `NavigationModel`
- `SessionNode -> TabNode -> PaneNode`
- Stable `NodeId` keys for session, tab, and pane nodes (`tab_position` is preserved as node metadata)
- `VisibleItem` flatten output for UI consumption
- `PreviewTarget` and `SelectionAction` resolution from selected `NodeId`
- `SourceFreshness` propagation (`runtime`, `subscription`, `cache`, `stale`, `error`)

Tabs and panes are session-scoped inputs (`SessionScopedData`) so tab/pane records are always tied to a concrete `session_name`.

## M2 Behavior

- Flattening returns one visible list in tree order: session -> tabs -> panes.
- The tree is fully expanded in M2 (no collapse-state model yet).
- Filtering is case-insensitive substring matching over:
  - session: name/state/age
  - tab: name/position
  - pane: title/command/cwd/kind
- Pane IDs are disambiguated with pane kind (`terminal` vs `plugin`) in `NodeId`.
- Tab/pane lookup for preview/action resolution is identity-first (`session + tab_id` and `session + tab_id + pane_id + pane_kind`) and tolerates `tab_position` reordering across refreshes.
- Plugin or non-selectable panes remain visible but resolve to `PreviewTarget::Unavailable` and `SelectionAction::NoAction`.

## M2 Non-Goals

- No TUI rendering (`ratatui`/`crossterm`)
- No preview rendering or streaming
- No process execution from the navigation model
- No selection execution wiring
- No cache parser implementation

## Expected M3 Consumption

The M3 TUI layer should consume `flatten_visible` / `flatten_filtered` as the list source, then call:

- `resolve_preview_target` for selected-row preview routing
- `resolve_selection_action` for enter-key action routing

This keeps navigation-state modeling separate from preview rendering and command execution.
