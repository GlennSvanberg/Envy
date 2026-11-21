# AGENTS.md

Notes for anyone changing Envy. The UI, code comments, and docs are English.

## What this is

Envy is a desktop editor for `.env` files. It edits files in place, inside folders the user adds, and groups those files by git repo. It is a window, not a service: no tray, no MCP server, no CLI yet. Closing the window quits.

## Layout

```
Cargo.toml                 workspace, release profile (thin LTO, strip, opt-level z)
crates/envy-core/          no window, no gpui
  document.rs              lossless parse and serialize
  session.rs               one open file: undo stack, dirty flag, atomic save
  discover.rs              parallel walk, path cache, git grouping, env file names
  diff.rs paste.rs         compare files, Vercel-style paste
  scaffold.rs              .env.example -> new file, secrets cleared
  search.rs config.rs      key-name scoring (unused by the window), config.toml
  deeplink.rs hotkey.rs    envy:// and the global chord
  future.rs                named, unimplemented seams (age, self-decrypting HTML, git exposure)
crates/envy/               the window
  main.rs                  gpui-kit app, AllAssets (full icon set), window icon, quit saves
  build.rs                 embeds assets/icon/envy.ico into envy.exe on Windows
assets/icon/               logo sources, PNG sizes, envy.ico, README headers
  view.rs                  layout, overlays, key handling
  actions.rs               key bindings (ctrl-, not cmd-)
  instance.rs protocol.rs  single instance and the URL scheme
  hotkey_os.rs             OS global hotkey
.github/workflows/ci.yml   Linux tests, Windows envy.exe artifact
```

`envy` depends on `envy-core`. Do not put window types in the core crate.

## Commands

```powershell
cargo test --workspace --locked
cargo build --release -p envy --locked
cargo fmt --all
```

CI runs the test command on `ubuntu-latest` and `cargo build --release -p envy --locked` on `windows-latest`, then uploads `target/release/envy.exe` as `envy-windows`.

Linux needs the GPUI packages listed in the workflow (`libfontconfig1-dev`, wayland, xkbcommon, x11, ssl, zstd, cmake, pkg-config). Debug builds already optimize the gpui crates; application code stays unoptimized.

## Threading rule

The UI thread only renders and handles input. Anything that can wait belongs on `cx.background_executor().spawn` (or a dedicated thread when the OS requires one), and the view is updated afterwards with `this.update`.

That covers the folder dialog (`cx.prompt_for_paths`, which runs the native dialog off the GPUI executor), folder scans, git `HEAD` and remote reads, reading and atomic-saving `.env` files, diff computation, config load and save, the clipboard-clear wait, URL-scheme registration, and the single-instance accept loop. The window polls those results; it does not call `recv` or `read` itself.

Two exceptions stay off that executor on purpose:

- The second process connects to `instance.port` and exits before GPUI starts, so it never opens a window.
- On Windows the global hotkey is registered on `envy-hotkey`, a thread that also pumps `WM_HOTKEY`. `RegisterHotKey` delivers to the thread that created the hidden window, so that call cannot live on the UI thread or on a pool thread that does not pump messages.

Do not call `rfd` or any other synchronous file dialog from a click handler. A nested modal loop on the GPUI thread freezes the window.

Cold start paints an empty window first. Config load and the path cache are read after that, still off the UI thread. If a cache exists, the sidebar shows those paths immediately, then `scan_paths` refreshes them in the background. The walk is `ignore`'s `WalkParallel`. It skips `is_skipped_dir` names (`node_modules`, `target`, `.git`, `bin`, `obj`, and the rest) and ignored directories, and it stops at `MAX_SCAN_DEPTH`. It does not read file contents. Gitignored files named `.env`, `.env.*`, or `*.env` are still listed. `fill_key_index` reads key names afterwards, and `attach_git_identity` reads git config and `HEAD`. Both stay on a background task. Found paths are written to `index-cache.toml` (paths only). A non-recursive `notify` watch on the added folders and project roots asks for another scan when an env file appears or disappears. There is no timer that rescans on its own.

## UI copy

UI copy speaks to the user about their task, never about the implementation. Do not mention threads, the window staying responsive, git internals, the config file format, or how a save is atomic. Status text is for what they can do next (`Select a file`, `Saved.`, `Add a folder to start`). A scan shows a count in the sidebar (`12 files`), not a waiting screen.

## Design rules

- One accent: the logo green (`#34D399` on a dark theme, `#047857` on a light theme). Selection, the active tab, focus, and search matches use it. Warning and danger stay for status.
- Never use a thick left-border bar as a selection or callout indicator. A selected row is a quiet fill and accent text. A focused area is a thin outline.
- UI copy talks about the task. No implementation talk, and no decorative filler.
- No command palette. Search is the sidebar field and the field above the keys.

## Constraints

- Icons first. A button that has a Lucide icon uses it (`gpui_kit::assets::IconName`) with a hover tooltip. Text labels are for rows, empty states, and settings that are not a single action. The icon font comes from `gpui_kit::assets::AllAssets` in `main.rs`. The default asset subset does not contain the icons we use.
- Never store values. Config, logs, and the two filter fields may hold folder paths, theme, the hotkey, project names, and key names. They must not hold secret values. Do not log `Entry::value`. Projects are filtered in the sidebar and keys are filtered above the variable list, by name only.
- The atomic-save sibling (`filename.envy-tmp`, next to the target) exists only during `write_atomic`. Delete it on failure. Do not treat it as a backup, and do not discover it as an env file (`is_env_filename` rejects the suffix). On Windows the replace uses `MoveFileExW` with `MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH`, because `std::fs::rename` does not replace an existing file there.
- Preserve comments and order. Untouched entries keep their raw line. `set_value` rewrites only the entries it changes. `serialize` is the way to write a document; do not add a `Display` that calls it (that recurses).
- A committed edit is written immediately, off the UI thread, through the lossless writer. That includes a value, a renamed key, a toggle, and an added or removed row. Plain text is written after a short pause. Undo and redo stay, and the result is written the same way. Switching files or closing the window writes anything not yet on disk. `OpenFile` is dirty when `serialize()` differs from `clean`. There is no Save button and no unsaved dot.
- Two files are compared by selecting two tabs (Ctrl+click, Shift+click, or Shift+Enter). The first tab is the left column, the second is the right. `d` opens that view with the example (or the next file) beside the open file. Each row can copy a value either direction. "Copy missing" copies keys that exist on only one side. Copying into either file writes that file.
- Favorites and recently opened projects are stored as paths on `Config` (`favorites`, `recent_projects`). They never store values. The sidebar order is favorites, then recent, then name. Recent order is applied at launch. Moving through the list does not reorder it. Pinning does.
- `cmd` in a GPUI keystroke is the Super key. Bind `ctrl-` explicitly. `KeyBinding::new` does not resolve `secondary-`.
- While a cell is open, Tab moves key → value → the next row's key, and Shift+Tab moves back. Enter commits and closes the field. The row stays selected, so Up and Down move between rows and Enter opens the value again. Esc cancels. Arrows, Home, and End stay in the field. The root captures Tab and Enter so the field does not take them. Otherwise Tab and Shift+Tab move between the project list, the file tabs, and the keys. A new key still uses Tab to leave the name and Enter to commit the row. An open key list does not take Enter; a click picks a key.
- A production file is marked with a small red dot, not a bar.
- Light and dark both have to stay readable. Prefer theme tokens over extra hard-coded colors. Rows keep a fixed height.

## Config

`%APPDATA%\envy\config.toml` on Windows. `$XDG_CONFIG_HOME/envy` or `~/.config/envy` elsewhere. The instance port file sits beside it and is not config.
