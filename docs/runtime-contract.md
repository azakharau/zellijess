# Runtime Discovery Contract (M5)

## Commands Executed

The runtime discovery module executes these read-only Zellij commands:

1. `zellij list-sessions --no-formatting`
2. `zellij --session <session> action list-tabs --json --all` (reliable multi-session path)
3. `zellij --session <session> action list-panes --json --all` (reliable multi-session path)
4. `zellij --session <session> action dump-screen --pane-id <id> --ansi` (one-shot pane snapshot)
5. `zellij --session <session> subscribe --pane-id <id> --ansi --format json --scrollback 0` (selected-pane near-live updates)
6. `zellij action list-tabs --json --all` (best-effort current-context fallback)
7. `zellij action list-panes --json --all` (best-effort current-context fallback)

For near-live pane preview, M5 uses command (5) for selected-pane live updates and keeps command (4) as startup/fallback behavior.

All commands are executed through an injectable command-runner abstraction that captures:

- stdout
- stderr
- exit code

## Expected Output Shape

### `list-sessions --no-formatting`

- Plain text, one session per line.
- Preferred line shape: `<session-name> [Created <relative-age>]`.
- Suffix-bearing lines are accepted after the closing `]` (for example: `(EXITED - attach to resurrect)`, `(current)`, `(active)`).
- Parsed session metadata includes `name`, optional `created_age`, and derived state.
- `[Created ...]` with no suffix after `]` derives `active`.
- Known suffixes currently mapped are `(current)` => `current`, `(EXITED - attach to resurrect)` => `exited`, and `(active)` => `active`.
- Unrecognized non-empty suffixes after `]` remain `unknown`.
- If `[Created ...]` is not present, the parser keeps the full line as the session name, leaves `created_age` empty, and sets state to `unknown`.

### `--session <session> action list-tabs --json --all`

- JSON array of tab objects.
- Parser currently expects typed tab fields including: `position`, `name`, `active`, `tab_id`, and optional metadata fields (`viewport_*`, `display_area_*`, bell/swap/floating flags, and pane counts).
- `panes_to_hide` accepts either an array of pane ids (`[]`/`[id, ...]`) or a legacy numeric `0`, which is normalized to an empty list.
- Parser also accepts NDJSON-style one-object-per-line output as a fallback.

### `--session <session> action list-panes --json --all`

- JSON array of pane objects.
- Parser currently expects typed pane fields including: `id`, plugin/focus/fullscreen/floating flags, geometry fields, tab linkage (`tab_id`, `tab_position`, `tab_name`), and optional command/path fields (`pane_command`, `pane_cwd`).
- `index_in_pane_group` accepts either a numeric index or an empty object form (`{}`), where `{}` is normalized to `None`.
- Parser also accepts NDJSON-style one-object-per-line output as a fallback.

### `--session <session> action dump-screen --pane-id <id> --ansi`

- Returns pane snapshot content as ANSI-capable text output.
- Non-zero exit status is surfaced as a command failure (command, exit code, stderr).
- Empty stdout or ANSI-reset-only output (`\x1b[0m`, `\x1b[m`, plus whitespace/newlines) maps to an explicit empty snapshot state.
- Non-empty output maps to ready snapshot content.

### `--session <session> subscribe --pane-id <id> --ansi --format json --scrollback 0`

- Command is scoped to one selected pane (`--pane-id <id>`) in one selected session (`--session <session>`).
- Output is consumed line-by-line as JSON events.
- Only `{"event":"pane_update", ...}` frames are used for preview updates.
- Snapshot body is built by joining `scrollback` and `viewport` lines with `\n`.
- ANSI-reset-only output is normalized to empty snapshot state.

### Selected-pane near-live refresh (M5)

- Startup path performs one-shot `dump-screen` first.
- Live path starts a selected-pane subscribe worker using scoped `subscribe` command.
- If subscribe exits/errors, worker emits one-shot `dump-screen` fallback snapshot/error.
- Worker count is bounded to one active preview worker.
- Worker is cancelled on selection change and app exit.
- Updates are coalesced through a bounded channel; stale updates are ignored by request/session/pane identity.
- The startup path remains one-shot snapshot first, then selected-pane near-live refresh.

### Unscoped `action list-tabs/list-panes --json --all`

- Treated as best-effort current-context fallback only.
- Observed multi-session runtime behavior: command can return exit code `0` while emitting non-JSON diagnostic output (stdout/stderr) that asks for a session name.
- Runtime discovery classifies this case as a runtime/session-targeting diagnostic rather than generic JSON schema drift.
- Session-scoped commands are the reliable basis for the M1+ navigation model.

## Known Failure Modes

1. **Command missing / not executable**
   - Command runner returns an I/O failure.
2. **Command exits non-zero**
   - Runtime discovery surfaces command, exit code, and stderr.
   - Typical cause: tab/pane actions called when not in a compatible Zellij context.
3. **Runtime/session-targeting diagnostic with exit code 0**
   - Unscoped tab/pane commands may emit session-targeting diagnostic text while still exiting with status `0`.
   - This is surfaced distinctly from parse/schema drift.
4. **Output shape drift**
   - JSON parse errors are surfaced as parse failures.
   - Session line format drift can trigger `InvalidSessionLine` if a `[Created ...` fragment is malformed (for example, missing closing `]`).

## Intentionally Not Handled Yet

- Broad multi-pane subscriptions.
- Persistent preview daemon behavior.
