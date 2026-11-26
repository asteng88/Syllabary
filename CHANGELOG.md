# Changelog

All notable changes to **Syllabary** are documented in this file.

## [0.2.7] - 2026-11-26

### Added
- **Animated Studio Splash Screen (`splash.rs`):**
  - Added a custom animated obsidian studio splash screen on startup featuring breathing amber/magenta/indigo aurora glows, typographic drafting grid & print registration marks, diagonal glass entrance sheen, glowing Syllabary emblem, amber-to-magenta animated progress bar, and author signature (`Created by asteng88 — station88retro.com`, with clickable link).
  - Smoothly fades in and out automatically, or dismisses immediately on click / `Esc` / `Enter` / `Space`.
  - Added author attribution (`asteng88 — station88retro.com`) and a **"Show Splash Screen"** replay button to **Help → About Syllabary**.

---

## [0.2.6] - 2026-11-26

### Added
- **Non-Blocking Background Document Loading & Live Progress Bar:**
  - Moved document reading, parsing, PDF page interpretation, `hayro` page setup, and editable PDF text block extraction onto a background worker thread (`open_path_async` / `poll_doc_load` in `app.rs`, `read_with_progress` / `extract_editable_blocks_with_progress` in `pdf_import.rs`, `attach_prepared_pdf_view` in `editor.rs`).
  - Added a centered, theme-aware loading progress card with a spinner, filename, animated percentage progress bar, live per-page status readout, and elapsed timer so large documents (especially multi-page PDFs) no longer freeze the window or trigger OS `(Not Responding)` warnings.

---

## [0.2.5] - 2026-11-26

### Added
- **Full Linux & Fedora Port:**
  - Added a pure-Rust offline Hunspell spell-checking backend (`spellbook`) on Linux (`spell.rs`) that automatically loads system dictionaries from `/usr/share/hunspell`, `/usr/share/myspell`, and `~/.local/share/hunspell` and persists custom words to `~/.config/Syllabary/custom_dict.txt`.
  - Added Linux/Fedora font discovery (`fonts.rs`), including `~/.local/share/fonts`, `/usr/share/syllabary/fonts`, Fedora emoji/symbol paths (`google-noto-emoji-color-fonts`, `dejavu-sans-fonts`, `gdouros-symbola`), and automatic bundled open-source font fallbacks when Windows system fonts are absent.
  - Implemented per-user Linux XDG file associations (`file_assoc.rs`) and updated `syllabary.desktop` with `.wpx` and `.odt` MIME types.
  - Guarded Windows-only resource compilation in `app/build.rs` and added clear read-only install messaging in `updater.rs` for RPM/Flatpak installs.
- **Cross-Platform Syllabary Installer (`installer/`):**
  - Built a standalone native GUI installer (`syllabary-installer.exe` for Windows, `syllabary-installer-linux` for Linux/Fedora) with a dedicated Obsidian + Emerald download-badge installer icon.
  - Downloads the latest GitHub release over HTTPS with live progress and SHA-256 verification (plus a local-binary fallback).
  - Installs to standard per-user paths (`%LOCALAPPDATA%\Programs\Syllabary` on Windows, `~/.local/bin/syllabary` on Linux with `chmod +x` and SELinux `restorecon`), registers Syllabary in **Applications → Office** on Linux and **Start Menu / Installed Apps** on Windows, optionally creates a Desktop shortcut and default file associations, and supports one-click updating and uninstalling.

---

## [0.2.4] - 2026-11-25

### Added
- **OpenDocument Text (`.odt`) Full Import, Edit & Export Support:**
  - Added a pure-Rust ODF 1.2 (ISO/IEC 26300) `.odt` reader and writer (`odt.rs`) supporting document metadata (`meta.xml`), page dimensions, margins, named styles, headers/footers (`styles.xml`), headings, paragraphs, character formatting, hyperlinks, bulleted & numbered lists, tables, and embedded images (`content.xml` + `Pictures/`).
  - Integrated `.odt` across **Open**, **Save / Save As**, **File → Download → OpenDocument Text (.odt)**, the Command Palette, and Windows file associations.
- **Settings Side Panel & One-Click Default App Registration:**
  - Moved **Settings / Preferences** (`Tools → Settings / Preferences…`) out of a modal dialog into a docked, scrollable right-hand studio side panel (`RightPanel::Settings`).
  - Added a **Default App & File Associations** section (`file_assoc.rs`) with per-extension checkboxes (`.wpx`, `.docx`, `.odt`, `.pdf`, `.gdoc`, `.md`, `.rtf`, `.txt`), **Select All / Clear**, live **Default** status badges, and a **"Set Syllabary as Default App"** button that registers per-user (`HKCU`, no admin required) ProgIDs, icons, `Capabilities`, and `RegisteredApplications`, then refreshes Explorer via `SHChangeNotify`.
  - Added an **"Open Windows Default Apps…"** shortcut (`ms-settings:defaultapps`) and automatic detection of Windows `UserChoice`-protected extensions (such as `.pdf` or `.docx`).

---

## [0.2.3] - 2026-11-25

### Added
- **Independent Headers & Footers Side Panel (No Popups):**
  - Moved **Headers & Footers** and **Page Setup** out of modal popups into docked right-hand studio side panels (`RightPanel::HeadersFooters` and `RightPanel::PageSetup`) with live on-page updates and a 3-page preview readout.
  - Added independent page-scope rules (`HfScope`: **All pages**, **First page only**, **Odd pages only**, **Even pages only**) and independent **Different first page** toggles (`header_first` / `footer_first`) for headers and footers.
  - Added one-click quick-insert chips and dynamic auto-filling tokens: **Page #** (`{page}`), **Page X of Y** (`Page {page} of {pages}`), **Strictly Confidential**, **Confidential — Internal Use Only**, **DRAFT**, **Date** (`{date}`), and **Title** (`{title}`), rendered consistently across the live canvas, print rasterizer, and `krilla` vector PDF export.
- **ModernRust UI Library Integration (`P:\ModernRust`):**
  - Integrated the standalone **`ModernRust`** crate for Windows 11 Fluent components: animated semantic `ToastStack` (with countdown progress bar), Fluent `PromptBox` confirmations, gliding `[ Read | Edit ]` segmented pill, `fluent_switch` toggles, quick-insert `chip` pills, `side_panel_header`, and docked `edge_collapse_tab`.
  - Upgraded **53 toolbar, chrome, and editorial icons** (including **B**, *I*, U, S, Text Color, Highlight, Lists, Indents, Line Spacing, Clear Format, Clipboard, Format Painter, Spell Check, Image, Star, Quote, Code, Save, Print, PDF, Table, Headers, Moon, Sun, Settings, Undo/Redo, Align) to ModernRust's persistent `24×24` SVG + `resvg` HiDPI renderer.

### Fixed
- **Toolbar Pill Centering:** Balanced the toolbar container's vertical margins (`Margin::symmetric(14, 6)`) so the floating toolbar pill sits vertically centered between its top and bottom border lines.

---

## [0.2.2] - 2026-11-25

### Added
- **In-Place PDF Text Editing (`Read` / `Edit` Modes):**
  - Added an explicit segmented **`[ Read | Edit ]`** toggle in the top header bar for opened PDFs.
  - **Read mode** renders the clean, pixel-accurate PDF via `hayro` (including any in-place edits, with no editing chrome).
  - **Edit mode** overlays clickable text blocks extracted directly from the PDF's own positioned text spans (`pdf_import::extract_editable_blocks`) with sampled background patching.
  - **PDF Editor Right Side Panel** (with docked `‹`/`›` collapse tab): **+ Add Text Box**, **Undo / Redo**, font size (`A−` / `DragValue` / `A+`), **Bold**, *Italic*, **Alignment** (`Left`, `Center`, `Right`, **`Justify`**), text color swatches, exact `X` / `Y` / `Width` controls, **Delete Text Block**, and **Convert to Reflowable Doc** escape hatch.
  - **Direct Canvas Manipulation:** Floating **`⋮⋮ Move`** drag handle and right-edge **width resize handle** on selected PDF text boxes with live `X`/`Y`/`W` coordinate badges and full Undo/Redo (`Ctrl+Z` / `Ctrl+Y`).
  - **Vector-Text PDF Saving (`Ctrl+S` / Export PDF):** `pdf::export_edited_pdf` uses `krilla` to combine the high-DPI original PDF background, clean background patches over moved/edited/deleted blocks, and real searchable, wrapped, aligned/justified vector text, automatically refreshing Read mode on save.
- **Table Cell Math Target Picker:**
  - Added three ways to change the calculation target to any cell in the table: a **Target Cell** dropdown, a **`🎯 Pick Target in Grid`** toggle, and right-clicking any cell in the preview grid, with automatic self-reference prevention and `ABOVE`/`LEFT` adjustment.
- **Explicit State Machines (Reliability & Architecture):**
  - Refactored four implicit state machines into explicit Rust enums: `CanvasDrag` (canvas pointer/resize/cell selection), `CloseFlow` (tab-close & app-quit confirmations), `RightPanel` (mutually exclusive Comments & Version History drawers), and `BlockKind` (canonical paragraph block classification), plus `PdfMode` and `PdfBlockDrag`.

### Fixed
- **Table Editing & Navigation:** Fixed `Tab`/`Shift+Tab` grid navigation (with automatic row append at the last cell), `Up`/`Down` arrow movement across rows, `Backspace`/`Delete` cell-boundary guards, and `doc_id` focus lock so the cursor never jumps unexpectedly to adjacent cells.
- **Window Unmaximize Layout Collapse:** Fixed the top toolbar expanding vertically and collapsing the document canvas when transitioning from fullscreen/maximized to a smaller window.
- **PDF Edit Deselect Crash:** Fixed a re-entrant `egui` `Fonts` lock panic when clicking or double-clicking outside an edited/added PDF text box by laying out galleys before painting.
- **Chrome & Icons:** Removed the redundant top-left pen icon and cleaned up the dark-mode crescent moon icon (`Icon::Moon`) to render smoothly without concave polygon artifacts.

---

## [0.2.1] - 2026-11-24

### Added
- **DOCX / PDF / GDOC / Markdown Accuracy Overhaul:** Word `hhea`/`OS/2` line metrics with metric-compatible font twins, full pagination rules (`keep_next`, `keep_lines`, `widow_control`, `cant_split`, `is_header`), `pulldown-cmark` CommonMark/GFM reader, `krilla` true vector PDF export, and `hayro` pixel-accurate Exact PDF View.
- **App Icon Suite & Windows File Associations:** Obsidian Serif `S` application icon and 7 Cobalt Nib document icons (`.wpx`, `.docx`, `.pdf`, `.gdoc`, `.md`, `.rtf`, `.txt`) with Windows registration script.
- **Single-Source `VERSION` & Direct GitHub Auto-Updater:** Built-in GitHub Releases update checker, SHA-256 verification, and seamless self-update.
