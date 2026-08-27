# TUI Shell (M5)

## Behavior

M5 keeps the M4 shell boundaries and upgrades active-pane preview to near-live updates with bounded worker lifecycle.

- Left panel renders `NavigationModel::flatten_visible` / `flatten_filtered` rows.
- Right panel routes preview from `resolve_preview_target`:
  - session/tab selections render summary details,
  - terminal pane selections perform an initial one-shot `dump-screen` preview, then bounded near-live refresh for the selected pane only,
  - pane preview supports loading, ready, empty, error, stale, and unavailable states.
- Bottom line shows compact mode/filter/action status.
- `Enter` resolves and stages `resolve_selection_action` output only; no execution occurs.
- Stale snapshot/live results are rejected so older pane requests do not overwrite newer selection previews.
- At most one background preview worker runs; it is cancelled on selection change and app exit.

## Keyboard

- `j` / `Down`: move selection down.
- `k` / `Up`: move selection up.
- `/`: enter filter mode.
- `Esc`:
  - clear filter when query is non-empty,
  - leave filter mode when query is empty,
  - quit when already in navigation mode with empty filter.
- `Enter`: compute and stage pending `SelectionAction` (without executing).
- `q`: quit in navigation mode.
- `Ctrl-c`: quit from any mode.
- In filter mode, `q` is treated as a normal query character.

## Non-goals (still out of scope)

- No broad multi-pane subscriptions.
- No persistent preview daemon.
- No selection action execution.
- No Zellij config mutation.
- No nested terminal rendering.

## Notes

- Demo mode remains fixture-backed for navigation data while terminal-pane preview still uses runtime `dump-screen`; runtime command failures surface as non-blocking preview errors.
- In non-interactive environments (no TTY), `demo` loads fixture data and exits with a skip message instead of failing raw-mode setup.
- Background preview updates use native `zellij --session <session> subscribe --pane-id <id> --ansi --format json --scrollback 0` for the selected pane only.
- The startup path remains one-shot `dump-screen` first; if subscribe exits/errors for the selected pane, preview falls back to a one-shot `dump-screen` update for bounded recovery.
