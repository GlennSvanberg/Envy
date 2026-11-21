# Envy Design System

Design specification and token reference for Envy, a keyboard-first desktop editor for `.env` files built with GPUI and `gpui-component`.

---

## 1. Design Principles

1. **Calm, architectural craft.** Modeled on tools like Zed, Linear, and Raycast. Clear surface hierarchy, hairline borders, quiet chrome, and high-contrast data. No decorative gradients, no glow effects, no emoji, and no thick left-border accent bars.
2. **One accent, used sparingly.** Logo green (`#34D399` in dark mode, `#047857` in light mode) is the single brand accent. It marks active focus, active selection, the active file tab, pinned projects, and search matches. Semantic amber and red are reserved strictly for status.
3. **Strict 4 / 8 px spatial rhythm.** Every container height, gap, padding, and corner radius sits on a 4 px grid (with 1–2 px reserved exclusively for hairlines and micro-indicators).
4. **Pixel-locked alignment.** Table headers, data rows, inline editors, and compare columns share identical width constants so opening an edit or toggling an action never shifts surrounding text.
5. **Effortless keyboard navigation.** Every zone (`Projects`, `Tabs`, `Keys`) has clear, quiet visual focus feedback, automatic scroll-into-view, and concise keycap hints in the footer.
6. **Task-focused copy.** Every label, tooltip, and status message talks about the user's files and keys, never about threads, config files, git internals, or implementation details.

---

## 2. Color Tokens

Envy separates **chrome** (title bar, sidebar, tab strip, footer) from the **canvas** (editor and compare workspace) using subtle elevation steps rather than flat pure black or pure white.

### 2.1 Surfaces & Borders

| Token | Dark | Light | Usage |
| --- | --- | --- | --- |
| `bg.canvas` | `#111214` | `#FFFFFF` | Main key list, compare canvas, active file tab |
| `bg.chrome` | `#16171A` | `#F5F6F8` | Title bar, sidebar, tab bar background, toolbar, footer |
| `bg.surface` | `#1C1E22` | `#FFFFFF` | Modal dialogs, elevated cards, dropdowns |
| `bg.input` | `#191B1F` | `#FFFFFF` | Search inputs and inline edit fields |
| `bg.kbd` | `#202228` | `#ECEEF2` | Keycap badges and neutral summary chips |
| `bg.hover` | `#1E2127` | `#EEF0F4` | Row hover, tab hover, ghost button hover |
| `bg.selected_muted` | `#1E2127` | `#E8EBF0` | Selected row when its zone is not focused |
| `bg.selected_accent` | `#34D399` @ 9% | `#047857` @ 8% | Quiet tint for the selected row in the focused zone |
| `border.hairline` | `#23262C` | `#E4E7EC` | Pane dividers, bar separators, table headers |
| `border.control` | `#2F333C` | `#CFD5DF` | Inputs, keycaps, outline buttons, modal borders |
| `backdrop` | `#090A0C` @ 64% | `#111318` @ 32% | Modal overlay scrim |

### 2.2 Foreground & Ink

| Token | Dark | Light | Usage |
| --- | --- | --- | --- |
| `fg.primary` | `#ECEEF2` | `#111318` | Project names, key names, selected rows, revealed values, headings |
| `fg.secondary` | `#9AA0AA` | `#4B5362` | Inactive tabs, masked values, toolbar icons, neutral chips, descriptions |
| `fg.tertiary` | `#7B8394` | `#687080` | Placeholders, empty-value labels, column headers, paths, branch names |

### 2.3 Brand Accent & Semantic Status

| Token | Dark | Light | Usage |
| --- | --- | --- | --- |
| `accent` | `#34D399` | `#047857` | Active tab underline, pinned star, primary button, active toggles, search highlight |
| `accent.ring` | `#34D399` @ 26% | `#047857` @ 28% | Quiet 1 px focused-row hairline, active toggle border, input focus border |
| `accent.subtle` | `#34D399` @ 11% | `#047857` @ 9% | Active/toggled icon button fill, primary icon well, compare badges |
| `status.warning` | `#F59E0B` | `#B45309` | Missing-vs-example pill text, diff-changed glyph |
| `status.warning_dot` | `#F59E0B` | `#D97706` | Missing-keys drift indicator |
| `status.warning_bg` | `#F59E0B` @ 9% | `#D97706` @ 9% | Missing-vs-example pill background |
| `status.warning_border` | `#F59E0B` @ 22% | `#D97706` @ 22% | Missing-vs-example pill border |
| `status.prod` | `#E5484D` | `#DC2626` | 6 px production file marker dot |
| `status.danger` | `#F87171` | `#DC2626` | Save/load errors, external conflict accent, diff right-only glyph |
| `status.danger_bg` | `#EF4444` @ 10% | `#DC2626` @ 8% | External disk change banner background |
| `status.danger_border` | `#EF4444` @ 24% | `#DC2626` @ 22% | External disk change banner border |

### 2.4 Compare Row Tints

Equal rows stay completely plain (transparent). Differing rows receive only a whisper-quiet tint so the colored `12 px` status icon carries the semantic cue without visual clutter.

| Diff State | Icon (`12 px`) | Icon Color | Row Tint (Dark) | Row Tint (Light) |
| --- | --- | --- | --- | --- |
| `Same` | `Equal` | `fg.tertiary` | transparent | transparent |
| `Different` | `EqualNot` | `status.warning` | `#F59E0B` @ 4.5% | `#D97706` @ 5% |
| `OnlyLeft` | `Plus` | `accent` | `#34D399` @ 3.5% | `#047857` @ 3.5% |
| `OnlyRight` | `Minus` | `status.danger` | `#EF4444` @ 3.5% | `#DC2626` @ 3.5% |

---

## 3. Typography Scale

Two font families only:
- **UI Sans:** System UI font (Segoe UI on Windows, Inter / sans fallback on Linux).
- **Monospace:** System mono font (Cascadia Mono / Consolas on Windows, JetBrains Mono / DejaVu Sans Mono on Linux) for filenames, key names, values, paths, and keycaps.

| Role | Family | Size | Line Height | Weight | Usage |
| --- | --- | --- | --- | --- | --- |
| `empty.title` | UI Sans | `16 px` | `22 px` | SemiBold (600) | Empty state heading |
| `modal.title` | UI Sans | `14 px` | `20 px` | SemiBold (600) | Overlay dialog headings, window title |
| `row.primary` | UI Sans | `13 px` | `18 px` | Medium (500) / Regular (400) | Project names, modal body copy |
| `code.row` | Monospace | `13 px` | `18 px` | Regular (400) / Medium (500) | Env keys, values, file tabs, diff values |
| `ui.compact` | UI Sans | `12 px` | `16 px` | Regular (400) / Medium (500) | Search inputs, buttons, status pills, settings labels |
| `micro` | UI Sans / Mono | `11 px` | `14 px` | Medium (500) / Regular (400) | Column headers, project paths, footer path, keycaps |

**Clipping invariant:** Never assign a fixed box height smaller than the text element's `line_height`. Two-line rows always size their parent height to accommodate both line heights plus inter-line gap and vertical breathing room.

---

## 4. Spacing, Density & Radii

### 4.1 Spacing Scale
- `2 px` — micro gap (between project name and branch line; hairline accents)
- `4 px` — tight group gap (between sibling toolbar icons, row actions, keycap + label)
- `8 px` — default component gap and compact padding (sidebar insets, tab bar padding, toolbar group dividers)
- `12 px` — section horizontal padding (toolbar, footer, panel headers)
- `16 px` — spacious padding (table horizontal gutter, modal body padding)
- `24 px` — empty state and modal vertical breathing room

### 4.2 Fixed Component Dimensions

| Region | Dimension | Notes |
| --- | --- | --- |
| Window default / min | `1120 × 720 px` / `780 × 500 px` | Centered on launch |
| Title bar | `36 px` height | Matches tab bar and sidebar header; flat `bg.chrome`, brand left + active project/branch breadcrumb |
| Sidebar width | `256 px` fixed | Separated from main pane by 1 px `border.hairline` |
| Sidebar header | `36 px` height | Title + file/project count or scan indicator + Add folder / Refresh |
| Sidebar row | `40 px` | 18 px name + 14 px path. Sidebar width `200–480 px`, default `256 px` |
| File tab bar | `36 px` height | Full-width strip above toolbar |
| Workspace toolbar | `40 px` height | Search/summary left, grouped icon buttons right |
| Column header strip | `28 px` height | Edge-to-edge strip matching exact column widths of rows below |
| Section divider row | `28 px` height | Discreet group label (`11 px`, `fg.tertiary`) + `1 px` `border.hairline` rule |
| Key row | `32 px` | Mono key (fits the longest name, `160–560 px`) and value (`flex_1`). A comment on the key is a tooltip only |
| Compare row | `32 px` fixed height | Single-line, edge-to-edge 4-column aligned grid (`256 px` key, `56 px` centered copy gutter) |
| Footer / Status bar | `28 px` height | Short project-relative path or status left, keycap hints right |
| Toolbar icon button | `28 × 28 px` | `15 px` Lucide icon, `6 px` radius |
| Inline row action button | `24 × 24 px` | `14 px` Lucide icon, `4 px` radius |
| Compact search input | `28 px` height | `6 px` radius, `Search` icon prefix, clearable |

### 4.3 Border Radii
- `4 px` — keycaps (`Kbd`), inline row icon buttons, micro badges (`L` / `R`)
- `6 px` — sidebar rows, key rows, toolbar buttons, search inputs, compare rows
- `10 px` — modal cards (Settings, Shortcuts, Add folder, Paste preview)
- `999 px` — status dots and compact pill badges

---

## 5. Layout & Component Specifications

### 5.1 Sidebar (`Projects` Zone)
- **Header (`36 px`):** Left shows `PROJECTS` (`11 px`, medium, `fg.tertiary`) plus either a subtle scanning spinner + live file count (`12 files`) while scanning, or the project count when idle. Right has two `28 × 28 px` ghost icon buttons: `Refresh` (`RefreshCw`, tooltip `"Rescan folders · Ctrl+R"`) and `Add folder` (`FolderPlus`, tooltip `"Add folder · Ctrl+O"`).
- **Search field:** `8 px` horizontal padding, `8 px` bottom padding. `Input` (`28 px`, `Search` icon prefix, `cleanable(true)`, placeholder `"Find a project"`). Hidden when no folders have been added.
- **Project list:** Edge-to-edge scrollable container (`project_scroll`) with no outer box border.
- **Width:** `256 px` until the user drags the handle on the sidebar's right edge (`200–480 px`). The width is remembered. The handle is a `1 px` hairline in a `7 px` hit area; the pointer is `col-resize`, and the line takes the accent color while hovered.
- **Sections:** `FAVORITES` then `ALL PROJECTS`, each a `11 px` medium `fg.tertiary` label. A hairline sits above All projects when Favorites is also shown. An empty section is omitted, including while the list is filtered. Favorites keep pin order. All projects are recent, then name.
- **Project row (`40 px`, `mx(8 px)`, `px(8 px)`, `rounded(6 px)`):**
  - Center (`flex_1`, `min_w(0)`):
    - Line 1: Project name (`13 px / 18 px`, `truncate()`, `fg.primary` with `FontWeight::MEDIUM` when selected). Focused selection uses quiet `bg.selected_accent` fill + `1 px` `accent.ring` hairline border on the row itself; unfocused selection uses neutral `bg.selected_muted` with no border.
    - Line 2: The project path (`11 px / 14 px`, `fg.tertiary`). Windows shows a drive letter and backslashes, without a `\\?\` prefix. Elsewhere the path uses slashes and shortens the home directory to `~`. When the path does not fit the sidebar, characters drop from the start (`…\glenn\braian`) and the full native path is the hover tooltip.
  - Right: `24 × 24 px` Pin/Star icon button (`StarFill` in `accent` when pinned; `Star` in `fg.tertiary` visible only on row hover when unpinned).
  - The search field matches the project name and the path. The branch is not shown on the row. Missing keys vs `.env.example` are not marked here.

### 5.2 File Tab Bar (`Tabs` Zone)
- Height `36 px`, background `bg.chrome`, `border_b_1` (`border.hairline`), horizontal scroll if many files.
- Each tab (`h(31 px)`, `px(12 px)`, `gap(6 px)`, `items_center`, `font_family(mono)`, `text_size(12.5 px)`):
  - **Active tab (edit mode):** `bg.canvas` surface, `fg.primary` medium text, `2 px` bottom accent bar (`accent`), subtle left/right `1 px` `border.hairline` seams so it physically connects to the workspace below.
  - **Compared tabs (compare mode):** Both selected tabs sit on `bg.canvas` with `2 px` bottom accent bar and a tiny `4 px`-radius mono badge (`L` on the left file, `R` on the right file) in `accent.subtle` / `accent`.
  - **Inactive tab:** Transparent background, `fg.secondary`, hover fill `bg.hover` with `fg.primary`.
  - **Keyboard focus (`zone == Zone::Tabs`):** The focused cursor tab displays a crisp `1 px` `accent.ring` outline and subtle `accent.subtle` fill.
  - **Indicators:**
    - Production file: `6 px` red dot (`status.prod`) before the filename.
    - Missing keys vs `.env.example`: subtle `5 px` amber dot (`status.warning_dot` @ 65%) after the filename with tooltip `"Missing keys vs .env.example"`.

### 5.3 Workspace Toolbar
- Height `40 px`, `px(12 px)`, `bg.canvas`, `border_b_1` (`border.hairline`), `items_center`, `justify_between`.
- **Left region (`gap(8 px)`):**
  - *Edit mode (table):*
    - Key filter `Input` (`w(220 px)`, `h(28 px)`, `Search` icon prefix, `cleanable(true)`, placeholder `"Find a key"`).
    - Key count (`12 px`, `fg.tertiary`): `"6 keys"` (or `"2 of 6"` while filtering).
    - Missing-keys drift pill (shown only when `missing.len() > 0`): calm interactive amber pill (`bg: status.warning_bg`, `border: 1 px status.warning_border`, `text: status.warning`, `h(24 px)`, `px(8 px)`, `rounded(999 px)`, `12 px` icon + `"2 missing vs .env.example"`). Clicking opens Compare with `.env.example`.
  - *Edit mode (plain text):*
    - Quiet notice pill (`bg.kbd`, `1 px border.hairline`, `Eye` icon + `"Plain text · values visible"`) and line count.
  - *Compare mode:*
    - Close compare icon button (`X`, tooltip `"Close compare · Esc"`).
    - Neutral summary chips (`bg.kbd`, `1 px border.hairline`, `fg.secondary` text) each pairing a small `11 px` colored Lucide glyph (`EqualNot`, `Plus`, `Minus`, or `Check`) with the count (`2 differ`, `3 left only`, `6 right only`, or `Files match`).
    - Cycle second file icon button (`ArrowLeftRight`, tooltip `"Compare another file · ]"`), shown when project has ≥ 3 files.
- **Right region (`gap(4 px)` within groups, `1 px × 16 px` `border.hairline` dividers with `mx(4 px)` between groups):**
  - *Edit group:*
    - `Copy` — Copy all enabled keys as `KEY=value` lines
    - `Plus` — New file in the current project (asks for a name, default `.env`, or the next free name)
    - `Undo2` — Undo (`Ctrl+Z`), disabled state when `!can_undo`
    - `Redo2` — Redo (`Ctrl+Y`), disabled state when `!can_redo`
  - *View & file group:*
    - `Eye` / `EyeOff` — Show/Hide all values (`A`), selected tint when active (disabled in plain text view)
    - `FileCode` / `List` — Switch between Table and Plain text view (`Ctrl+E`), selected tint when in plain text view, preference remembered in config
    - `Columns2` — Compare two files (`D`), selected tint when active, disabled when `< 2` files
    - `FilePlus` — New `.env.local` from `.env.example` (`Ctrl+Shift+N`), disabled when no example exists
  - *Utility group:*
    - `Keyboard` — Keyboard shortcuts (`?`)
    - `Settings` — Settings (`Ctrl+,`)

### 5.4 Key List (`Keys` Zone)
- Edge-to-edge scrollable table with no outer frame border, flowing cleanly across the canvas like Zed panels.
- **External disk change banner (conditional):** Compact `36 px` row with `status.danger_bg` fill, `border_b_1` (`status.danger_border`), `TriangleAlert` icon, concise copy `"This file changed on disk."`, and two inline compact buttons: `Reload` (`R`) and `Keep mine` (`Esc`).
- **Column header (`28 px`, `px(16 px)`, `border_b_1`, `bg.chrome` @ 55%):**
  - `KEY` — width fits the longest key in the open file (`160–560 px`, about `8.2 px` per character plus the toggle). Dragging the divider on the header's right edge sets a width that is remembered and used for every file. The pointer is `col-resize`.
  - `VALUE` — `flex_1`, `min_w(0)`
  - Actions spacer — `w(64 px)`, `flex_shrink_0`
- **Key rows (`32 px`, `mx(8 px)`, `px(7 px)` + `1 px` border = `16 px` inner edge, `rounded(6 px)`):**
  - Subtle hover (`bg.hover`), selected unfocused (`bg.selected_muted`, no border), selected focused (`bg.selected_accent` + `1 px accent.ring`, `fg.primary` text).
  - **Key column:** Leading `24 × 24 px` on/off toggle button (`ToggleRight` in `accent` for active entries, `ToggleLeft` in `fg.secondary` for commented-out/disabled entries; distinct `accent.subtle` + `accent.ring` hover state; tooltip `"Turn off · Ctrl+Space"` / `"Turn on · Ctrl+Space"`). Always visible on disabled rows; shown on active rows only when hovered or selected+focused to keep the table calm. When both an active and disabled line exist for the same key, the disabled row sits directly beneath the active row with a `10 px` indent and `11 px` `CornerDownRight` indicator (never merged). Key name in mono (`13 px / 18 px`, `fg.primary`, or `fg.tertiary` when disabled), optional `export ` prefix in `fg.tertiary`. The name is truncated only when the column cannot grow any further; the hover tooltip is the full key, and a comment directly above the key is included there and nowhere else on the row. Clicking the key name renames it. Clicking a value commits the open field and edits that value, with the text selected so typing replaces it. Tab and Shift+Tab move to the next and previous value. Enter commits and moves to the next value. Esc cancels. Up and Down do the same as Shift+Tab and Tab. The caret stays in a value field, including on the first and last row. The value is masked again when the field closes.
  - **Section dividers (`28 px`):** Standalone comments and decorated section headers (`# --- Database ---`) render as discreet non-selectable dividers (`11 px` `fg.tertiary` label + `1 px` `border.hairline`) in file order.
  - **Value column:** `flex_1`, `min_w(0)`, `mono`, `13 px / 18 px`, `truncate()`, dimmed when the row is disabled.
    - Masked: capped at **12 dots** (`•` repeated `len.min(12)`), rendered in `fg.secondary` so rows stay calm and never overflow or leak exact long secret lengths.
    - Revealed: `fg.primary`, full value tooltip on hover.
    - Empty: `"empty"` in `fg.tertiary`.
  - **Actions column (`w(64 px)`, `flex_shrink_0`, `justify_end`, `gap(4 px)`):** `Copy` (`Copy`) and `Reveal/Hide` (`Eye` / `EyeOff`) `24 × 24 px` icon buttons. Visible when the row is selected (focused) or hovered.
  - **Tail action:** A quiet `+ Add key` ghost row (`32 px`, `fg.tertiary`) sits below the last key so the table finishes cleanly above open canvas space.
  - **Inline editing (`Value`, `Rename`, `New`) & key-name autocomplete:** Renders inside the key column, the flexible value column, and the `64 px` actions column with a monospace `Input` and no outer double-border. Once at least 1 character is typed into a new or renamed key name, a quiet compact list (max 5 keys, matching the key input width, no source badges or footer bar, lightly emphasizing the matched text) appears directly below ranked internally by sibling env files missing from this file, other discovered projects, then common env key names. Navigable with `↑`/`↓`/`Tab`/`Enter`/`Esc` or mouse click; picking a key never prefills or hints values.
  - **File name:** `+` in the toolbar asks for a new env file name in the tab bar. The field starts as `.env`, or the next free name (`.env.local`, `.env.development`, `.env.dev`, then `.env.2`, `.env.3`, …) when that name exists. The name is selected so it can be changed before Enter. Double-clicking a tab renames that file in place. Enter confirms, Esc cancels, and a name that is taken or not an env file is explained on the line under the field.

### 5.4b Plain Text Editor View
- Toggled via the toolbar `FileCode` / `List` icon button or `Ctrl+E`. Choice is persisted in config (`editor_view = "table" | "text"`).
- Edge-to-edge monospace code editor with line numbers, `.env` syntax highlighting (keys, values, strings, numbers, booleans, `${VAR}` interpolations, comments, disabled lines), and native text undo/redo. Edits are written after a short pause.
- Unsaved changes stay losslessly synced when switching between Table and Plain text views; invalid lines stay verbatim and surface through the footer's unreadable-lines indicator.

### 5.5 Two-Column Compare View
- Replaces the key list area while keeping the file tab bar, toolbar, and footer intact. Edge-to-edge with no outer boxed outline.
- **Single aligned header (`36 px`, `px(16 px)`, `border_b_1`, `bg.chrome` @ 65%):**
  - Col 1 (`w(256 px)`, `flex_shrink_0`, `pr(12 px)`): `"KEY"` (`11 px`, `fg.tertiary`).
  - Col 2 (`flex_1`, `min_w(0)`, `h_flex`, `justify_between`, `items_center`): Left filename (mono, `12 px`, `fg.primary`, `L` badge) + compact outline `"← Copy missing"` button (shown when right-only keys exist, tooltip `"Copy N missing into <left>"`).
  - Col 3 (`w(56 px)`, `flex_shrink_0`): centered `12 px` `ArrowLeftRight` icon marking the copy gutter axis.
  - Col 4 (`flex_1`, `min_w(0)`, `h_flex`, `justify_between`, `items_center`): Right filename (mono, `12 px`, `fg.primary`, `R` badge) + compact outline `"→ Copy missing"` button (shown when left-only keys exist, tooltip `"Copy N missing into <right>"`).
- **Compare rows (`32 px` fixed height, `mx(8 px)`, `px(7 px)` + `1 px` border = `16 px` inner edge, `rounded(6 px)`):**
  - Col 1 (`w(256 px)`, `flex_shrink_0`, `pr(12 px)`, `h_flex`, `items_center`, `gap(8 px)`): fixed `14 px` container with `12 px` Lucide status icon (`Equal`, `EqualNot`, `Plus`, `Minus`, or neutral `ToggleLeft` when a key is present but commented out/off — never flagged as missing) + mono key name (`13 px / 18 px`, `truncate()`, tooltip with full key).
  - Col 2 (`flex_1`, `min_w(0)`, `pr(8 px)`): Left value (with neutral `off` micro-badge and dimmed text when disabled, capped 12-dot mask, revealed text, or `—` in `fg.tertiary` when absent).
  - Col 3 (`w(56 px)`, `flex_shrink_0`, `h_flex`, `items_center`, `justify_center`, `gap(2 px)`): One tidy centered copy gutter — copying across preserves active/disabled state.
  - Col 4 (`flex_1`, `min_w(0)`, `pl(8 px)`): Right value (with neutral `off` micro-badge and dimmed text when disabled).
  - Keyboard support: `↑`/`↓` moves the compare row cursor (with subtle `1 px` `accent.ring` highlight + scroll-into-view), `Space` reveals/hides that key's values on both sides, `A` reveals/hides all, `]` cycles the right file, `Esc` closes compare.

### 5.6 Footer / Status Bar
- Height `28 px`, `px(12 px)`, `bg.chrome`, `border_t_1` (`border.hairline`), `items_center`, `justify_between`.
- **Left (`flex_1`, `min_w(0)`, `gap(8 px)`):**
  - Active transient status message (`11.5 px`, `fg.secondary`). A committed edit may show `Saved.` briefly, then the line returns to the short project-relative path (`project/relative/.env`, `11 px`, mono, `fg.tertiary`, `truncate()`, with full absolute path in the hover tooltip).
  - Neutral clickable unreadable-lines note (shown only when the open file has lines that are neither comment, blank, nor assignment): e.g. `"2 lines Envy can't read"` (`bg.kbd`, `border.hairline`, `fg.secondary`, zero red/warning styling). Opens a read-only modal listing exact 1-based line numbers and raw lines.
- **Right (`flex_shrink_0`, `gap(12 px)`):**
  - 3–4 context-sensitive shortcut hints composed of a micro keycap (`bg.kbd`, `1 px` `border.control`, `rounded(4 px)`, `px(5 px)`, `py(1 px)`, `10 px` mono, `fg.secondary`) + short label (`11 px`, `fg.tertiary`).

### 5.7 Overlays & Empty / Loading States
- All overlays sit inside a full-window flex container (`items_center`, `justify_center`) over the dimmed scrim (`backdrop`) so dialogs are always centered regardless of window size.
- Modal cards use `bg.surface`, `border_1` (`border.control`), `rounded(10 px)`, `shadow_lg()`, a dedicated header (`title` + `X` close icon button), and `16 px` padding.
- **Shortcuts (`?`):** `620 px` wide, 2 balanced columns, clean action labels on the left and styled keycaps on the right.
- **Settings (`Ctrl+,`):** `480 px` wide, structured rows for Theme (3-button segmented picker: System / Light / Dark), Clipboard auto-clear toggle, Global hotkey input, and Managed folders list with `Trash` buttons and `Add folder` button.
- **Empty state (no folders):** Centered calm layout with a `48 × 48 px` `accent.subtle` rounded icon container (`FolderPlus` in `accent`), `16 px` SemiBold heading `"Add a folder"`, single-sentence `13 px` muted copy, and a real primary `"Add folder"` button paired with a `Ctrl+O` keycap.
- **Loading / Booting state:** Minimal centered muted indicator (`LoaderCircle` + `"Starting"`); disappears as soon as cached or scanned paths arrive.

---

## 6. Interaction States

| Control | Rest | Hover | Active / Selected | Focused | Disabled |
| --- | --- | --- | --- | --- | --- |
| Icon button | transparent, `fg.secondary` | `bg.hover`, `fg.primary` | `accent.subtle`, `accent` | `1 px` `accent.ring` | `fg.tertiary` @ 45%, non-interactive |
| List row | transparent, `fg.primary` | `bg.hover` | `bg.selected_accent` + `accent` (focused zone) / `bg.selected_muted` (unfocused) | Zone `1 px` `accent.ring` | n/a |
| File tab | transparent, `fg.secondary` | `bg.hover`, `fg.primary` | `bg.canvas`, `fg.primary`, `2 px` bottom `accent` | `1 px` `accent.ring` + `accent.subtle` | n/a |
| Search input | `bg.input`, `border.control` | `border.control` | — | `1 px` `accent.ring` | `fg.tertiary` |
| Missing-keys pill | `status.warning_bg`, `status.warning_border` | slightly stronger tint | opens Compare | `1 px` `accent.ring` | hidden when 0 missing |

---

## 7. Compliance with `AGENTS.md`

- **UI thread discipline:** No blocking I/O, scans, saves, or dialogs run on the UI thread; all background work stays on `cx.background_executor().spawn`.
- **No stored values:** Filter inputs, config, cache, and logs never retain secret values.
- **Icons first:** Toolbar and row icon buttons use Lucide icons from `gpui_kit::assets::AllAssets` with descriptive tooltips (and keybinding hints).
- **No thick left-border bars:** Selection uses quiet fills and accent text; zone focus uses a quiet `1 px` outline.
- **Light & dark parity:** Every token has explicit dark and light values with verified contrast.
