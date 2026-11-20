# Envy

A small, keyboard-first editor for `.env` files. It reads and writes the files where they already live. It does not copy them, upload them, or put their values in config or logs.

Envy is a Windows 11 app. The window is [GPUI](https://github.com/zed-industries/zed) through [gpui-kit](https://github.com/longbridge/gpui-component) 0.10. Parsing, discovery, diff, and file IO live in the `envy-core` crate so a CLI can be added later without a second model of a `.env` file.

## Build on Windows

Install the [MSVC toolchain](https://rustup.rs/) (`rustup default stable-x86_64-pc-windows-msvc`) and [CMake](https://cmake.org/). GPUI links native code, so `cl.exe` and `cmake` need to be on `PATH` (the "Desktop development with C++" Visual Studio workload is enough).

```powershell
git clone https://github.com/GlennSvanberg/Envy.git
cd Envy
cargo run -p envy --release
```

The release profile uses thin LTO and strips symbols. The binary is `target\release\envy.exe`. Closing the window quits the process. There is no tray icon.

On first launch Envy registers the `envy://` URL scheme for the current user (`HKCU\Software\Classes\envy`). A second launch forwards its link to the running process and exits.

Config is `%APPDATA%\envy\config.toml`. It stores folders, theme, the global hotkey, and optional project names. It never stores environment values.

The same workspace builds on Linux. That needs the GPUI packages: `libfontconfig1-dev`, `libwayland-dev`, `libxkbcommon-dev`, `libx11-dev`, `libx11-xcb-dev`, `libxcb1-dev`, `libssl-dev`, `libzstd-dev`, `cmake`, `pkg-config`. The global hotkey uses X11 on Linux and does not register under Wayland. The folder picker is Windows-only; elsewhere you type the path.

## Using it

Add a folder (`Ctrl+O`). Envy walks that folder, skips dependency directories, and groups `.env*` files by git repository. The sidebar shows the repo name (from `origin`, otherwise the folder name), the current branch, and a red dot when a file is missing keys that `.env.example` has.

| Key | Action |
| --- | --- |
| `j` / `k` or `↑` / `↓` | Move between projects or keys |
| `h` / `l` or `←` / `→` | Previous / next file tab |
| `Tab` | Switch between the project list and the keys |
| `Enter` | Edit the value. In a dialog, confirm |
| `C` | Copy the value. Cleared from the clipboard after 30s unless you turn that off |
| `Space` | Show or hide the selected value |
| `A` | Show or hide every value |
| `N` | New key |
| `F2` | Rename the key |
| `X`, `Delete`, or `Backspace` | Delete the key (Enter confirms) |
| `Ctrl+Z` / `Ctrl+Shift+Z` | Undo / redo, then write the file |
| `Ctrl+S` | Save |
| `Ctrl+K` | Search projects, files, and key names |
| `Ctrl+V` | Paste a block of `KEY=value` lines |
| `D` | Open or close the diff panel |
| `]` | Compare against the next file in the project |
| `Ctrl+Shift+N` | Create `.env.local` from `.env.example` |
| `Ctrl+O` | Add a folder |
| `Ctrl+,` | Settings |
| `Ctrl+R` | Rescan folders |
| `R` | Reload after the file changed on disk |
| `Esc` | Cancel, or keep your copy when the file changed on disk |
| `Ctrl+Alt+E` | Raise the window from anywhere (change the chord in settings) |

Values are masked (a dot per character) until you reveal them. Files whose name has a `prod` or `production` segment get a thin red tab accent. Color elsewhere means something needs attention: a missing key, or a value that differs.

Pasting several `KEY=value` lines (the block Vercel's env UI copies) opens a preview of what will be added or overwritten. Enter applies it. A single line pastes into the field you are editing.

Each committed edit is written straight into the existing file. Envy does not write a temp file, because that would be a second copy of the secrets. A crash in the middle of a write can truncate the file.

If the file changes outside Envy, the status line says so. `R` reloads. `Esc` keeps the buffer you have open.

## Deeplinks

```
envy://project/<repo-name>
envy://project/<repo-name>/<file>
envy://project/<repo-name>/<file>?key=KEY
```

`<repo-name>` is the name shown in the sidebar. `<file>` is the filename, such as `.env.local`. Names are percent-encoded when they contain spaces or `?`. If Envy is already running, the new process hands the URL over and exits.

## Layout of the repo

- `crates/envy-core` — parser, diff, discovery, paste, scaffold, config, search. No window.
- `crates/envy` — the GPUI window, hotkey, single instance, and URL scheme.

`envy-core` names, and does not implement, encrypted sharing (age, and a self-decrypting HTML file) and a warning when a `.env` is not gitignored or is in git history. There is no MCP server and no CLI yet.

## Tests

```powershell
cargo test --workspace
```

The core tests cover a byte-for-byte parse round-trip, editing one key without touching the others, Vercel-style paste splitting, the diff, and creating a file from an example with secrets cleared and comments kept.
