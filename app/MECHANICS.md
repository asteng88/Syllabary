# Mechanics sheet — modern rebuild of the 1999 word processor

Everything here was read from the original program (`wordpro.exe`, its English
language library `wpenc70.dll`, and its data files) or measured from the
captured window. Values marked **estimated** were looked for and not found.

## Menus (wpenc70.dll, extended menu resource 1002)

Top level, in order: File, Edit, View, Create, Page, Text, Window, Help
(the screenshot shows exactly these for an empty document; Frame / Table /
Columns only appear when such an object is selected).

Kept (modernised):
- File: New Document (101), Open… Ctrl+O (790), Close (147), Save Ctrl+S (103),
  Save As… (104), Import/Export… (152) → Export as Rich Text / Plain Text,
  Print… (111) → not available yet, Document Properties (1629), User Setup →
  Preferences (162), SmartCorrect Setup (1505), Exit (113), Last Open Files (2094).
- Edit: Undo Ctrl+Z (151), Redo (156, "Undo/Redo Special"), Cut/Copy/Paste
  (153/154/155), Go To… Ctrl+G (452), Find & Replace Text… Ctrl+F (451),
  Proofing Tools → Word Count (1974), Select All.
- View: Layout (210), Draft (211), Full Page (201), Standard 100% (203),
  Zoom To → Margin Width (778), Page Width (779), 75% (796), 150% (797),
  200% (204), Other… (1673); Show/Hide → SmartIcons (208), Status Bar (265),
  Ruler (207), Headers & Footers (775), Clean Screen (233), marks.
- Create: Page break, Date/Time, Symbol, Bulleted / numbered list,
  Footnote-style "Comment note" left out.
- Page: Page Properties (381), Page Size, Margins, Orientation, Background,
  Header, Footer, Insert Page Number (356), Insert Page Break Ctrl+Enter (358).
- Text: Font & Color (251), Bullets & Numbers (285), Named Styles (1633),
  Normal (253), Attributes Bold Ctrl+B / Italic Ctrl+I / Underline Ctrl+U
  (254/255/256), Enlarge Text F4 (252), Reduce Text Shift+F4 (245),
  Alignment Left Ctrl+L / Center Ctrl+E / Right Ctrl+R / Full Justify Ctrl+J
  (259–262), Indent (258) / Outdent (284), Insert Other → Date/Time (158),
  Symbol (1973); case change from cmddefs (ToggleAllUpper 128 / ToggleAllLower 129),
  Highlight (cmddefs HighLight 143), Strikethrough (ToggleStrikeThru 127),
  Superscript/Subscript (103/104).
- Window: New Window, Close Window, Close All → documents become tabs.
- Help: Keyboard shortcuts, About.

Removed on request (no scanning, no internet, no sharing):
TWAIN Image → Acquire Image / Select Source (2116/2113/2115, `TWAIN_32.DLL`
at 0x8ac708), TeamMail (1637), TeamReview (602), TeamConsolidate (500),
TeamSecurity (600), Internet submenu (1957: HTML export, open/save to
internet, web tools, setup), Hyperlink (1935), InfoBus data link (883),
Show/Hide Internet Tools (1979), Lotus Internet Support (1616), Year 2000 (556).
The rebuild makes no network connections at all.

## Defaults

| Rule | Value | Source |
|---|---|---|
| Default font | Times New Roman | status bar in capture; fallback rule FUN_0058ac4f; the person confirmed |
| Default size | 12 pt | status bar in capture; the person confirmed |
| Default paragraph style name | "Default Text" | status bar in capture |
| Font fallback | fixed pitch → Courier New; swiss family (0x20) → Arial; otherwise → Times New Roman | FUN_0058ac4f (0x58ac4f) |
| New document name | "Untitled" + counter in base 10 → "Untitled1" | FUN_0044e7a6 (0x44e7a6) with string 6053 |
| Page indicator | " Pg %d", only redrawn when the page changes | FUN_00452923 (0x452923) with string 6049 |
| Page number text | "Page %d" | string 10678 |
| Word count message | "The document contains %ld characters and %ld words." / "The selected text contains …" | strings 15723/15724 |
| Find options | Whole words only, Words starting with, Words ending with, Words containing | strings 10916–10919 |
| Find scope | Entire document | string 10929 |
| Units | Inches (in) / Points (pts) | strings 10766 / 10769 |
| Page size | 8.5 × 11 in (Letter) | measured: page 1004 px wide, margins 118/120 px → 8.5 in with 1 in margins |
| Margins | 1 in all round | measured from capture (estimated for top/bottom) |
| Zoom start | 100 % ("Standard (100%)") | menu 203; the person: always start at 100 % |
| Zoom range | 25 % – 200 % | the person's decision |
| Zoom presets | Margin Width, Page Width, 75 %, 100 %, 150 %, 200 %, Full Page | menu 1630 |
| SmartCorrect list | 170 English pairs (e.g. teh→the, 1/2→½) | `ltscorrt.reg`, key SmartCorrect\2.0\en |
| Timed save | on, every 10 minutes | command TimeSave (cmddefs 12); interval **estimated** |
| Undo depth | 100 steps | **estimated** |
| Enlarge / Reduce step | 2 pt (min 6, max 96) | **estimated** (no table found, unknown unk_44f0eda0475d11cd) |
| Font size list | 8 9 10 11 12 14 16 18 20 24 28 32 36 48 72 | **estimated** |

## Modern updates
- Tabs instead of MDI child windows; Ctrl+N new document (original Ctrl+N was "Normal";
  Normal moved to Ctrl+Space).
- Flat, line-style toolbar icons drawn as vector shapes; light and dark themes.
- Live word count and page count in the status bar; zoom slider.
- Redo (Ctrl+Y), Find & Replace with live match count, match case.
- Documents saved in a local `.wpx` file (JSON) on this computer only; export to RTF and TXT; text files open.

## Added in round 2 (common modern features)
- Spell check as you type (red wavy underline), right-click suggestions, Ignore All, Add to Dictionary,
  and a Check Spelling window (F7 or Ctrl+F2, the original's shortcut from menu item 453).
  Uses the dictionary that ships with Windows, offline; the user's own language first, then English.
- Right-click menu: spelling, cut/copy/paste, select all, clear formatting, change case.
- Format painter, line-spacing button, navigation pane listing Title and Heading 1-3.
- Export to PDF (laid out with the same line breaks at 72 points per inch, PDF standard fonts:
  serif → Times, sans → Helvetica, monospace → Courier). Print (Ctrl+P, the original's shortcut)
  makes a print-ready PDF and opens it in the computer's PDF viewer.
- Open and save Word .docx (text, bold/italic/underline/strike, size, font, colour, highlight,
  super/subscript, alignment, spacing, indents, bullets/numbers, headings, page size/margins, title/author).
- 17 free fonts bundled in fonts/ (SIL Open Font License): Lora, Merriweather, Libre Baskerville,
  EB Garamond, Playfair Display, Source Serif 4, Open Sans, Roboto, Lato, Inter, Montserrat, Nunito,
  Work Sans, JetBrains Mono, Source Code Pro, Caveat, Dancing Script.
- Font picker: search box, Recently used, then Serif / Sans Serif / Monospace / Handwriting,
  each name drawn in its own typeface; bundled ones marked "included".
- Cambria is hidden: Windows ships it as a font collection (.ttc) the text engine can't read.
