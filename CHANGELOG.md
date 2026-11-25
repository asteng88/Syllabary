# Changelog

All notable changes to **Syllabary** are documented in this file.

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
