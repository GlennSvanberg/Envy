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
| `bg.kbd` | `#202228` | `#ECEEF2` | Keycap badges in footer and shortcuts overlay |
| `bg.hover` | `#202329` | `#ECEEF2` | Row hover, tab hover, ghost button hover |
| `bg.selected_muted` | `#22252C` | `#E6E9EF` | Selected row when its zone is not focused |
| `bg.selected_accent` | `#34D399` @ 12% | `#047857` @ 10% | Selected row when its zone is focused |
| `border.hairline` | `#23262C` | `#E5E7EC` | Pane dividers, bar separators, table headers, row hairlines |
| `border.control` | `#2F333C` | `#D4D8E1` | Inputs, keycaps, cards, modal borders |
| `backdrop` | `#090A0C` @ 64% | `#111318` @ 32% | Modal overlay scrim |

### 2.2 Foreground & Ink

| Token | Dark | Light | Usage |
| --- | --- | --- | --- |
| `fg.primary` | `#ECEEF2` | `#111318` | Project names, key names, revealed values, headings |
| `fg.secondary` | `#9AA0AA` | `#5A616E` | Inactive tabs, masked values, toolbar icons, descriptions |
| `fg.tertiary` | `#7B8394` | `#707887` | Placeholders, empty-value labels, column headers, paths, branch names |

### 2.3 Brand Accent & Semantic Status

| Token | Dark | Light | Usage |
| --- | --- | --- | --- |
| `accent` | `#34D399` | `#047857` | Active tab indicator, focused selection label, pinned star, search highlight |
| `accent.ring` | `#34D399` @ 30% | `#047857` @ 32% | Quiet 1 px zone focus outline and input focus border |
| `accent.subtle` | `#34D399` @ 12% | `#047857` @ 10% | Active/toggled icon button fill, focused row fill, primary icon well |
| `status.warning` | `#F59E0B` | `#B45309` | Missing-vs-example pill text, unsaved dot, diff-changed indicator |
| `status.warning_dot` | `#F59E0B` | `#D97706` | 6 px unsaved dot and missing-keys drift indicator |
| `status.warning_bg` | `#F59E0B` @ 10% | `#D97706` @ 10% | Missing-vs-example pill background |
| `status.warning_border` | `#F59E0B` @ 24% | `#D97706` @ 24% | Missing-vs-example pill border |
| `status.prod` | `#E5484D` | `#DC2626` | 6 px production file marker dot |
| `status.danger` | `#F87171` | `#DC2626` | Save/load errors, external conflict accent, diff right-only glyph |
| `status.danger_bg` | `#EF4444` @ 12% | `#DC2626` @ 10% | External disk change banner background |
| `status.danger_border` | `#EF4444` @ 28% | `#DC2626` @ 26% | External disk change banner border |

### 2.4 Compare Row Tints

Equal rows stay completely transparent so differences stand out without visual noise.

| Diff State | Icon (`12 px`) | Icon Color | Row Tint (Dark) | Row Tint (Light) |
| --- | --- | --- | --- | --- |
| `Same` | `Equal` | `fg.tertiary` | transparent | transparent |
| `Different` | `EqualNot` | `status.warning` | `#F59E0B` @ 7% | `#D97706` @ 8% |
| `OnlyLeft` | `Plus` | `accent` | `#34D399` @ 6% | `#047857` @ 6% |
| `OnlyRight` | `Minus` | `status.danger` | `#EF4444` @ 6% | `#DC2626` @ 6% |

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
| `micro` | UI Sans / Mono | `11 px` | `14 px` | Medium (500) / Regular (400) | Column headers, git branch subline, footer path, keycaps |

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
| Title bar | `34 px` height | Native/GPUI `TITLE_BAR_HEIGHT` |
| Sidebar width | `256 px` fixed | Separated from main pane by 1 px `border.hairline` |
| Sidebar header | `38 px` height | Title + file/project count or scan indicator + Add folder / Refresh |
| Sidebar row | `44 px` fixed height | 18 px name + 2 px gap + 14 px branch = 34 px inside 44 px (5 px top/bottom). Single-line centered when branch is hidden. |
| File tab bar | `38 px` height | Full-width strip above toolbar |
| Workspace toolbar | `40 px` height | Search/summary left, grouped icon buttons right |
| Column header strip | `28 px` height | Matches exact column widths of rows below |
| Key row | `32 px` fixed height | Single-line, baseline-aligned mono key & value |
| Compare row | `34 px` fixed height | Single-line, 4-column aligned grid |
| Footer / Status bar | `28 px` height | Left status/path, right keycap hints |
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
- **Header (`38 px`):** Left shows `PROJECTS` (`11 px`, medium, `fg.tertiary`) plus either a subtle scanning spinner + live file count (`12 files`) while scanning, or the project count when idle. Right has two `28 × 28 px` ghost icon buttons: `Refresh` (`RefreshCw`, tooltip `"Rescan folders · Ctrl+R"`) and `Add folder` (`FolderPlus`, tooltip `"Add folder · Ctrl+O"`).
- **Search field:** `8 px` horizontal padding, `8 px` bottom padding. `Input` (`28 px`, `Search` icon prefix, `cleanable(true)`, placeholder `"Find a project"`).
- **Project list:** Scrollable container (`project_scroll`) with subtle `1 px` `accent.ring` border when `zone == Zone::Projects` (transparent border otherwise so layout never shifts).
- **Project row (`44 px` fixed height, `mx(6 px)`, `px(8 px)`, `rounded(6 px)`):**
  - Left: `6 px` status dot — amber (`status.warning_dot`) if any file in the project is missing keys vs `.env.example`, otherwise transparent spacer so all project names align cleanly on the same vertical axis.
  - Center (`flex_1`, `min_w(0)`):
    - Line 1: Project name (`13 px / 18 px`, `truncate()`, `accent` when selected in focused zone, `fg.primary` otherwise).
    - Line 2 (conditional): Rendered **only** when `branch` is a known git branch (never shown for `""`, `"unknown"`, `"—"`, `"-"` or `"…"`). Displays a `11 px` `GitBranch` icon + branch name (`11 px / 14 px`, `fg.tertiary`, `truncate()`). When hidden, Line 1 centers vertically inside the `44 px` row.
  - Right: `24 × 24 px` Pin/Star icon button (`Star` in `accent` when pinned; `Star` in `fg.tertiary` visible only on row hover when unpinned).

### 5.2 File Tab Bar (`Tabs` Zone)
- Height `38 px`, background `bg.chrome`, `border_b_1` (`border.hairline`), horizontal scroll if many files.
- Each tab (`h(37 px)`, `px(12 px)`, `gap(6 px)`, `items_center`, `font_family(mono)`, `text_size(12.5 px)`):
  - **Active tab (edit mode):** `bg.canvas` surface, `fg.primary` medium text, `2 px` bottom accent bar (`accent`), subtle left/right `1 px` `border.hairline` seams so it physically connects to the workspace below.
  - **Compared tabs (compare mode):** Both selected tabs sit on `bg.canvas` with `2 px` bottom accent bar and a tiny `4 px`-radius mono badge (`L` on the left file, `R` on the right file) in `accent.subtle` / `accent`.
  - **Inactive tab:** Transparent background, `fg.secondary`, hover fill `bg.hover` with `fg.primary`.
  - **Keyboard focus (`zone == Zone::Tabs`):** The focused cursor tab displays a crisp `1 px` `accent.ring` outline and subtle `accent.subtle` fill.
  - **Indicators:**
    - Production file: `6 px` red dot (`status.prod`) before the filename.
    - Dirty (unsaved open file): `6 px` amber dot (`status.warning_dot`) after the filename.
    - Missing keys vs `.env.example` (when not dirty): subtle `5 px` amber outline/dot (`status.warning_dot` @ 70%) after the filename with tooltip `"Missing keys vs .env.example"`.

### 5.3 Workspace Toolbar
- Height `40 px`, `px(12 px)`, `bg.canvas`, `border_b_1` (`border.hairline`), `items_center`, `justify_between`.
- **Left region (`gap(8 px)`):**
  - *Edit mode:*
    - Key filter `Input` (`w(240 px)`, `h(28 px)`, `Search` icon prefix, `cleanable(true)`, placeholder `"Find a key"`).
    - Key count (`12 px`, `fg.tertiary`): `"6 keys"` (or `"2 of 6"` while filtering).
    - Missing-keys drift pill (shown only when `missing.len() > 0`): calm interactive amber pill (`bg: status.warning_bg`, `border: 1 px status.warning_border`, `text: status.warning`, `h(24 px)`, `px(8 px)`, `rounded(999 px)`, `12 px` icon + `"2 missing vs .env.example"`). Clicking opens Compare with `.env.example`.
  - *Compare mode:*
    - Close compare icon button (`X`, tooltip `"Close compare · Esc"`).
    - Compare summary chips (`11 px`, quiet pills): counts of differing / left-only / right-only keys, or `"Files match"` when identical.
    - Cycle second file icon button (`ArrowLeftRight` / `RefreshCw`, tooltip `"Compare another file · ]"`), shown when project has ≥ 3 files.
- **Right region (`gap(4 px)` within groups, `1 px × 16 px` `border.hairline` dividers with `mx(4 px)` between groups):**
  - *Edit group:*
    - `Plus` — New key (`N`)
    - `Undo2` — Undo (`Ctrl+Z`), disabled state when `!can_undo`
    - `Redo2` — Redo (`Ctrl+Y`), disabled state when `!can_redo`
    - `Save` — Save (`Ctrl+S`), accented fill (`accent.subtle`, icon in `accent`) when dirty; muted when clean
  - *View & file group:*
    - `Eye` / `EyeOff` — Show/Hide all values (`A`), selected tint when active
    - `Columns2` — Compare two files (`D`), selected tint when active, disabled when `< 2` files
    - `FilePlus` — New `.env.local` from `.env.example` (`Ctrl+Shift+N`), disabled when no example exists
  - *Utility group:*
    - `Keyboard` — Keyboard shortcuts (`?`)
    - `Settings` — Settings (`Ctrl+,`)

### 5.4 Key List (`Keys` Zone)
- Container sits inside a `1 px` `accent.ring` focus border when `zone == Zone::Keys` (transparent otherwise).
- **External disk change banner (conditional):** Compact `36 px` row with `status.danger_bg` fill, `border_b_1` (`status.danger_border`), `TriangleAlert` icon, concise copy `"File changed on disk."`, and two inline compact buttons: `Reload` (`R`) and `Keep mine` (`Esc`).
- **Column header (`28 px`, `px(16 px)`, `border_b_1`, `bg.chrome` @ 50%):**
  - `KEY` — `w(260 px)`, `flex_shrink_0`, `pr(16 px)`
  - `VALUE` — `flex_1`, `min_w(0)`
  - Actions spacer — `w(64 px)`, `flex_shrink_0`
- **Key rows (`32 px` fixed height, `mx(8 px)`, `px(8 px)`, `rounded(6 px)`):**
  - Subtle hover (`bg.hover`), selected unfocused (`bg.selected_muted`), selected focused (`bg.selected_accent`).
  - **Key column:** `w(260 px)`, `flex_shrink_0`, `pr(16 px)`, `mono`, `13 px / 18 px`, `truncate()`, tooltip with full key name. `export ` prefix rendered in `fg.tertiary` if exported.
  - **Value column:** `flex_1`, `min_w(0)`, `mono`, `13 px / 18 px`, `truncate()`.
    - Masked: capped at **12 dots** (`•` repeated `len.min(12)`), rendered in `fg.secondary` so rows stay calm and never overflow or leak exact long secret lengths.
    - Revealed: `fg.primary`, full value tooltip on hover.
    - Empty: `"empty"` in `fg.tertiary`.
  - **Actions column (`w(64 px)`, `flex_shrink_0`, `justify_end`, `gap(4 px)`):** `Copy` (`Copy`) and `Reveal/Hide` (`Eye` / `EyeOff`) `24 × 24 px` icon buttons. Visible when the row is selected or hovered.
  - **Inline editing (`Value`, `Rename`, `New`):** Renders inside the exact same `260 px` / `flex_1` / `64 px` column geometry with a crisp `26 px` bordered input (`border.control` / `accent.ring`) and subtle `Enter`/`Esc` actions so nothing shifts horizontally.

### 5.5 Two-Column Compare View
- Replaces the key list area while keeping the file tab bar, toolbar, and footer intact.
- **Single aligned header (`36 px`, `px(16 px)`, `border_b_1`, `bg.chrome` @ 60%):**
  - Col 1 (`w(232 px)`, `flex_shrink_0`, `pr(12 px)`): `"KEY"` (`11 px`, `fg.tertiary`).
  - Col 2 (`flex_1`, `min_w(0)`, `h_flex`, `justify_between`, `items_center`): Left filename (mono, `12 px`, `fg.primary`, `L` badge) + compact `"← Copy missing"` button (enabled when right-only keys exist, tooltip `"Copy missing keys into <left>"`).
  - Col 3 (`w(60 px)`, `flex_shrink_0`): centered spacer aligning with the row copy gutter.
  - Col 4 (`flex_1`, `min_w(0)`, `h_flex`, `justify_between`, `items_center`): Right filename (mono, `12 px`, `fg.primary`, `R` badge) + compact `"Copy missing →"` button (enabled when left-only keys exist, tooltip `"Copy missing keys into <right>"`).
- **Compare rows (`34 px` fixed height, `mx(8 px)`, `px(8 px)`, `rounded(6 px)`):**
  - Col 1 (`w(232 px)`, `flex_shrink_0`, `pr(12 px)`, `h_flex`, `items_center`, `gap(8 px)`): fixed `14 px` diff glyph (`=`, `≠`, `+`, `−`) + mono key name (`13 px / 18 px`, `truncate()`, tooltip with full key).
  - Col 2 (`flex_1`, `min_w(0)`, `pr(8 px)`): Left value (capped 12-dot mask, revealed text, or `—` in `fg.tertiary` when absent).
  - Col 3 (`w(60 px)`, `flex_shrink_0`, `h_flex`, `items_center`, `justify_center`, `gap(4 px)`): **Always reserves two fixed `24 px` slots** — left slot for `→` (copy left → right, rendered when `row.left.is_some()` and `row.kind != Same`) and right slot for `←` (copy right → left, rendered when `row.right.is_some()` and `row.kind != Same`). When a slot is inactive, an empty `24 px` spacer holds its width so columns remain pixel-aligned on every row.
  - Col 4 (`flex_1`, `min_w(0)`, `pl(8 px)`): Right value (capped 12-dot mask, revealed text, or `—` in `fg.tertiary` when absent).
  - Keyboard support: `↑`/`↓` moves the compare row cursor (with subtle highlight + scroll-into-view), `Space` reveals/hides that key's values on both sides, `A` reveals/hides all, `]` cycles the right file, `Esc` closes compare.

### 5.6 Footer / Status Bar
- Height `28 px`, `px(12 px)`, `bg.chrome`, `border_t_1` (`border.hairline`), `items_center`, `justify_between`.
- **Left (`flex_1`, `min_w(0)`, `gap(8 px)`):**
  - Unsaved dot (`6 px` amber) when open file has unsaved edits.
  - Active transient status message (`11 px`, `fg.secondary`), or when idle, the open file path (`11 px`, mono, `fg.tertiary`, `truncate()`, full path in tooltip).
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
