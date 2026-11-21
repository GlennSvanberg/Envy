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
  discover.rs              walk folders, git grouping, which names count as env files
  diff.rs paste.rs         compare files, Vercel-style paste
  scaffold.rs              .env.example -> new file, secrets cleared
  search.rs config.rs      palette scoring, config.toml
  deeplink.rs hotkey.rs    envy:// and the global chord
  future.rs                named, unimplemented seams (age, self-decrypting HTML, git exposure)
crates/envy/               the window
  main.rs                  gpui-kit app, AllAssets (full icon set), quit saves
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

Cold start paints an empty window first. Config, the file walk, and git identity run after that, in that order. `discover_files` does not read git config; `attach_git_identity` does, and only in the background. The walk skips `node_modules`, `target`, `.git`, and the other names in `is_skipped_dir`, and it stops at `MAX_SCAN_DEPTH`.

## Design rules

- Icons first. A button that has a Lucide icon uses it (`gpui_kit::assets::IconName`) with a hover tooltip. Text labels are for rows, empty states, and settings that are not a single action. The icon font comes from `gpui_kit::assets::AllAssets` in `main.rs`. The default asset subset does not contain the icons we use.
- Never store values. Config, logs, search, and the palette may hold folder paths, theme, the hotkey, project names, and key names. They must not hold secret values. Do not log `Entry::value`.
- The atomic-save sibling (`filename.envy-tmp`, next to the target) exists only during `write_atomic`. Delete it on failure. Do not treat it as a backup, and do not discover it as an env file (`is_env_filename` rejects the suffix). On Windows the replace uses `MoveFileExW` with `MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH`, because `std::fs::rename` does not replace an existing file there.
- Preserve comments and order. Untouched entries keep their raw line. `set_value` rewrites only the entries it changes. `serialize` is the way to write a document; do not add a `Display` that calls it (that recurses).
- Edits are in memory until save. `OpenFile` is dirty when `serialize()` differs from `clean`. Undo and redo do not touch disk. Save on `Ctrl+S`, on switching files, and on window close.
- Copy missing keys into the file that is open, using the other file's values. The other direction would write secrets into an example file. The user can switch files and press the button again.
- `cmd` in a GPUI keystroke is the Super key. Bind `ctrl-` explicitly. `KeyBinding::new` does not resolve `secondary-`.
- Tab commits an inline edit. The root captures Tab while an editor is open so the input's indent binding does not take it.
- Production files get their red accent from a separate 2px bar. One `border_color` paints every edge, so it cannot also be the selection underline.
- Light and dark both have to stay readable. Prefer theme tokens (`background`, `foreground`, `muted_foreground`, `border`, `sidebar`, `danger`, `warning`, `success`, `info`) over hard-coded colors. Rows are short and corners are rounded; the window should feel closer to Raycast or Linear than to a form.

## Config

`%APPDATA%\envy\config.toml` on Windows. `$XDG_CONFIG_HOME/envy` or `~/.config/envy` elsewhere. The instance port file sits beside it and is not config.
