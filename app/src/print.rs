//! Cross-platform native printing for Windows (Win32 `PrintDlgW` + GDI `StartDocW`/`StretchDIBits`),
//! macOS (CUPS `lpstat`/`lp`/`lpr` + macOS system print sheet), and Linux (CUPS `lpstat`/`lp`/`lpr`).

use eframe::egui;

use crate::doc::Document;
use crate::fonts::FontBook;

/// Result of a native print operation.
#[derive(Clone, Debug)]
pub enum PrintOutcome {
    /// Job was spooled to the native printer subsystem.
    Printed(String),
    /// User cancelled the native print dialog.
    #[cfg_attr(not(target_os = "windows"), expect(dead_code, reason = "Constructed by Windows PrintDlgW cancellation"))]
    Cancelled,
    /// Native printing failed with a descriptive error.
    Error(String),
}

/// Duplex (two-sided) printing mode for CUPS (macOS & Linux).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum DuplexMode {
    #[default]
    OneSided,
    LongEdge,
    ShortEdge,
}

impl DuplexMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::OneSided => "One-Sided",
            Self::LongEdge => "Two-Sided (Long-Edge Binding)",
            Self::ShortEdge => "Two-Sided (Short-Edge Binding)",
        }
    }

    pub fn cups_option(self) -> &'static str {
        match self {
            Self::OneSided => "sides=one-sided",
            Self::LongEdge => "sides=two-sided-long-edge",
            Self::ShortEdge => "sides=two-sided-short-edge",
        }
    }
}

/// Interactive print settings used by the macOS / Linux native CUPS print dialog.
#[derive(Clone, Debug, PartialEq)]
pub struct PrintJobConfig {
    pub printers: Vec<String>,
    pub selected_printer: usize,
    pub all_pages: bool,
    pub from_page: usize,
    pub to_page: usize,
    pub total_pages: usize,
    pub copies: u32,
    pub color: bool,
    pub duplex: DuplexMode,
}

impl PrintJobConfig {
    pub fn discover(total_pages: usize) -> Self {
        let total = total_pages.max(1);
        let (mut printers, default_printer) = discover_cups_printers();
        if printers.is_empty() {
            printers.push("Default System Printer".into());
        }
        let selected_printer = default_printer
            .and_then(|def| printers.iter().position(|p| p == &def))
            .unwrap_or(0);
        Self {
            printers,
            selected_printer,
            all_pages: true,
            from_page: 1,
            to_page: total,
            total_pages: total,
            copies: 1,
            color: true,
            duplex: DuplexMode::OneSided,
        }
    }
}

/// Queries installed CUPS printers and the system default printer on macOS and Linux via `lpstat`.
pub fn discover_cups_printers() -> (Vec<String>, Option<String>) {
    let mut printers: Vec<String> = Vec::new();
    let mut default_printer: Option<String> = None;

    if let Ok(out) = std::process::Command::new("lpstat").args(["-p", "-d"]).output() {
        let text = String::from_utf8_lossy(&out.stdout);
        for line in text.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("printer ") {
                if let Some(name) = rest.split_whitespace().next() {
                    if !name.is_empty() && !printers.iter().any(|p| p == name) {
                        printers.push(name.to_string());
                    }
                }
            } else if let Some(def) = trimmed.strip_prefix("system default destination:") {
                let name = def.trim();
                if !name.is_empty() {
                    default_printer = Some(name.to_string());
                }
            }
        }
    }

    if printers.is_empty() {
        if let Ok(out) = std::process::Command::new("lpstat").arg("-a").output() {
            let text = String::from_utf8_lossy(&out.stdout);
            for line in text.lines() {
                if let Some(name) = line.split_whitespace().next() {
                    if !name.is_empty() && !printers.iter().any(|p| p == name) {
                        printers.push(name.to_string());
                    }
                }
            }
        }
    }

    (printers, default_printer)
}

/// Spools a calibrated WYSIWYG PDF to the native CUPS printing system (`lp` / `lpr`) on macOS & Linux.
pub fn submit_cups_print_job(ctx: &egui::Context, doc: &Document, book: &FontBook, title: &str, cfg: &PrintJobConfig) -> PrintOutcome {
    let range = if cfg.all_pages {
        None
    } else {
        let f = cfg.from_page.clamp(1, cfg.total_pages.max(1));
        let t = cfg.to_page.clamp(f, cfg.total_pages.max(1));
        Some((f, t))
    };
    let bytes = crate::pdf::export_page_range(ctx, doc, book, range);
    let dir = std::env::temp_dir().join("Syllabary");
    let _ = std::fs::create_dir_all(&dir);
    let safe_title: String = title
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' || c == ' ' { c } else { '_' })
        .collect();
    let pdf_path = dir.join(format!("{}.pdf", if safe_title.trim().is_empty() { "Document" } else { safe_title.trim() }));
    if let Err(e) = std::fs::write(&pdf_path, bytes) {
        return PrintOutcome::Error(format!("Couldn't write temporary print spool file: {e}"));
    }

    let printer_name = cfg.printers.get(cfg.selected_printer).map_or("", String::as_str);
    let explicit_printer = !printer_name.is_empty() && printer_name != "Default System Printer";

    let mut lp_cmd = std::process::Command::new("lp");
    if explicit_printer {
        lp_cmd.arg("-d").arg(printer_name);
    }
    lp_cmd
        .arg("-n")
        .arg(cfg.copies.max(1).to_string())
        .arg("-t")
        .arg(if title.is_empty() { "Syllabary Document" } else { title })
        .arg("-o")
        .arg(if cfg.color { "print-color-mode=color" } else { "print-color-mode=monochrome" })
        .arg("-o")
        .arg(if cfg.color { "ColorModel=RGB" } else { "ColorModel=Gray" })
        .arg("-o")
        .arg(cfg.duplex.cups_option())
        .arg(&pdf_path);

    match lp_cmd.output() {
        Ok(out) if out.status.success() => {
            let target = if explicit_printer { printer_name } else { "system default printer" };
            return PrintOutcome::Printed(format!("Sent print job to {target}."));
        }
        _ => {}
    }

    // Fallback to `lpr` (standard BSD/CUPS interface on macOS and Linux)
    let mut lpr_cmd = std::process::Command::new("lpr");
    if explicit_printer {
        lpr_cmd.arg("-P").arg(printer_name);
    }
    lpr_cmd
        .arg("-#")
        .arg(cfg.copies.max(1).to_string())
        .arg("-J")
        .arg(if title.is_empty() { "Syllabary Document" } else { title })
        .arg("-o")
        .arg(if cfg.color { "ColorModel=RGB" } else { "ColorModel=Gray" })
        .arg("-o")
        .arg(cfg.duplex.cups_option())
        .arg(&pdf_path);

    match lpr_cmd.output() {
        Ok(out) if out.status.success() => {
            let target = if explicit_printer { printer_name } else { "system default printer" };
            PrintOutcome::Printed(format!("Sent print job to {target}."))
        }
        Ok(out) => {
            let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
            PrintOutcome::Error(if err.is_empty() {
                "CUPS spooler returned an error. Check that a printer is configured in System Settings.".into()
            } else {
                format!("Print error: {err}")
            })
        }
        Err(e) => PrintOutcome::Error(format!("Could not invoke native print spooler (lp/lpr): {e}")),
    }
}

/// Opens the calibrated WYSIWYG PDF in the native OS viewer/print sheet (e.g. macOS Preview).
pub fn open_system_print_preview(ctx: &egui::Context, doc: &Document, book: &FontBook, title: &str) -> PrintOutcome {
    let bytes = crate::pdf::export(ctx, doc, book);
    let dir = std::env::temp_dir().join("Syllabary");
    let _ = std::fs::create_dir_all(&dir);
    let safe_title: String = title
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' || c == ' ' { c } else { '_' })
        .collect();
    let pdf_path = dir.join(format!("{}.pdf", if safe_title.trim().is_empty() { "Document" } else { safe_title.trim() }));
    if let Err(e) = std::fs::write(&pdf_path, bytes) {
        return PrintOutcome::Error(format!("Couldn't prepare print preview: {e}"));
    }

    #[cfg(target_os = "macos")]
    let res = std::process::Command::new("open").arg("-a").arg("Preview").arg(&pdf_path).spawn();
    #[cfg(target_os = "linux")]
    let res = std::process::Command::new("xdg-open").arg(&pdf_path).spawn();
    #[cfg(target_os = "windows")]
    let res = std::process::Command::new("explorer").arg(&pdf_path).spawn();

    match res {
        Ok(_) => PrintOutcome::Printed("Opened calibrated WYSIWYG PDF in system preview.".into()),
        Err(e) => PrintOutcome::Error(format!("Couldn't launch system preview: {e}")),
    }
}

/// Prints the document using the native Windows Print Dialog (`PrintDlgW`) and GDI device context (`HDC`).
#[cfg(target_os = "windows")]
pub fn print_native_windows(ctx: &egui::Context, doc: &Document, book: &FontBook, title: &str) -> PrintOutcome {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::GlobalFree;
    use windows::Win32::Graphics::Gdi::{
        DeleteDC, GetDeviceCaps, SetStretchBltMode, StretchDIBits, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HALFTONE,
        HORZRES, PHYSICALHEIGHT, PHYSICALOFFSETX, PHYSICALOFFSETY, PHYSICALWIDTH, SRCCOPY, VERTRES,
    };
    use windows::Win32::Storage::Xps::{EndDoc, EndPage, StartDocW, StartPage, DOCINFOW};
    use windows::Win32::UI::Controls::Dialogs::{
        PrintDlgW, PD_ALLPAGES, PD_NOSELECTION, PD_PAGENUMS, PD_RETURNDC, PD_USEDEVMODECOPIESANDCOLLATE, PRINTDLGW,
    };

    let lay = crate::layout::build(ctx, doc, book, 0.75, false);
    let total_pages = lay.pages.max(1);
    let max_page_u16 = total_pages.min(u16::MAX as usize) as u16;

    let mut pd = PRINTDLGW {
        lStructSize: core::mem::size_of::<PRINTDLGW>() as u32,
        Flags: PD_RETURNDC | PD_USEDEVMODECOPIESANDCOLLATE | PD_ALLPAGES | PD_NOSELECTION,
        nFromPage: 1,
        nToPage: max_page_u16,
        nMinPage: 1,
        nMaxPage: max_page_u16,
        nCopies: 1,
        ..Default::default()
    };

    // SAFETY: `pd` is a valid, properly sized `PRINTDLGW` initialized on the stack; `PrintDlgW`
    // is the standard Win32 modal print dialog API.
    let accepted = unsafe { PrintDlgW(&mut pd) }.as_bool();

    let free_globals = |pd: &PRINTDLGW| {
        if !pd.hDevMode.is_invalid() {
            // SAFETY: `hDevMode` was allocated by `PrintDlgW` and is no longer locked.
            let _ = unsafe { GlobalFree(Some(pd.hDevMode)) };
        }
        if !pd.hDevNames.is_invalid() {
            // SAFETY: `hDevNames` was allocated by `PrintDlgW` and is no longer locked.
            let _ = unsafe { GlobalFree(Some(pd.hDevNames)) };
        }
    };

    if !accepted || pd.hDC.is_invalid() {
        free_globals(&pd);
        return PrintOutcome::Cancelled;
    }

    let hdc = pd.hDC;
    let (first_page, last_page) = if (pd.Flags.0 & PD_PAGENUMS.0) != 0 {
        let f = (pd.nFromPage.max(1) as usize).min(total_pages);
        let t = (pd.nToPage.max(pd.nFromPage) as usize).min(total_pages);
        (f, t)
    } else {
        (1, total_pages)
    };
    let copies = (pd.nCopies.max(1) as usize).min(999);

    // Render high-DPI WYSIWYG page bitmaps (240 DPI base × screen DPI scale)
    let rgba_pages = crate::pdf::render_pages_rgba(ctx, doc, book, 2.5);

    // SAFETY: `hdc` is a valid printer device context returned by `PrintDlgW`.
    let (horz_res, vert_res, phys_w, phys_h, off_x, off_y) = unsafe {
        let hr = GetDeviceCaps(Some(hdc), HORZRES).max(1);
        let vr = GetDeviceCaps(Some(hdc), VERTRES).max(1);
        let pw = GetDeviceCaps(Some(hdc), PHYSICALWIDTH).max(hr);
        let ph = GetDeviceCaps(Some(hdc), PHYSICALHEIGHT).max(vr);
        let ox = GetDeviceCaps(Some(hdc), PHYSICALOFFSETX).max(0);
        let oy = GetDeviceCaps(Some(hdc), PHYSICALOFFSETY).max(0);
        (hr, vr, pw, ph, ox, oy)
    };

    let doc_title = if title.trim().is_empty() { "Syllabary Document" } else { title.trim() };
    let wide_title: Vec<u16> = doc_title.encode_utf16().chain(core::iter::once(0)).collect();
    let di = DOCINFOW {
        cbSize: core::mem::size_of::<DOCINFOW>() as i32,
        lpszDocName: PCWSTR(wide_title.as_ptr()),
        ..Default::default()
    };

    // SAFETY: `hdc` is valid and `di` points to a null-terminated UTF-16 document name that outlives `StartDocW`.
    let job_id = unsafe { StartDocW(hdc, &di) };
    if job_id <= 0 {
        // SAFETY: `hdc` was created by `PrintDlgW` and we own its lifetime.
        let _ = unsafe { DeleteDC(hdc) };
        free_globals(&pd);
        return PrintOutcome::Cancelled;
    }

    let mut pages_printed = 0usize;
    let mut print_err: Option<String> = None;

    'outer: for _ in 0..copies {
        for page_num in first_page..=last_page {
            let Some((img_w, img_h, rgba)) = rgba_pages.get(page_num - 1) else {
                continue;
            };
            let w = *img_w as i32;
            let h = *img_h as i32;
            if w <= 0 || h <= 0 {
                continue;
            }

            // Convert unmultiplied RGBA `[r, g, b, a]` to 32-bit GDI BGRA `[b, g, r, 255]`
            let mut bgra = Vec::with_capacity(rgba.len());
            for &[r, g, b, _] in rgba.as_chunks::<4>().0 {
                bgra.push(b);
                bgra.push(g);
                bgra.push(r);
                bgra.push(255);
            }

            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: core::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w,
                    biHeight: -h, // top-down DIB
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    biSizeImage: bgra.len() as u32,
                    ..Default::default()
                },
                ..Default::default()
            };

            // Determine destination rectangle on the physical page while respecting hardware printable bounds
            let min_margin_in = doc.page.margin_left.min(doc.page.margin_right).min(doc.page.margin_top).min(doc.page.margin_bottom);
            let doc_margin_px_x = (min_margin_in / doc.page.width.max(1.0) * phys_w as f32) as i32;
            let doc_margin_px_y = (min_margin_in / doc.page.height.max(1.0) * phys_h as f32) as i32;

            let (dst_x, dst_y, dst_w, dst_h) = if doc_margin_px_x >= off_x && doc_margin_px_y >= off_y {
                // Map 1:1 onto the physical sheet so 1" document margins measure exactly 1" on paper
                let scale = (phys_w as f64 / w as f64).min(phys_h as f64 / h as f64);
                let dw = ((w as f64 * scale).round() as i32).max(1);
                let dh = ((h as f64 * scale).round() as i32).max(1);
                let dx = (phys_w - dw) / 2 - off_x;
                let dy = (phys_h - dh) / 2 - off_y;
                (dx, dy, dw, dh)
            } else {
                // Fit within the hardware printable area so narrow-margin pages never clip
                let scale = (horz_res as f64 / w as f64).min(vert_res as f64 / h as f64);
                let dw = ((w as f64 * scale).round() as i32).max(1);
                let dh = ((h as f64 * scale).round() as i32).max(1);
                let dx = (horz_res - dw) / 2;
                let dy = (vert_res - dh) / 2;
                (dx, dy, dw, dh)
            };

            // SAFETY: `hdc` has an active print document started by `StartDocW`; `bgra` and `bmi`
            // are valid for the duration of `StretchDIBits`.
            unsafe {
                if StartPage(hdc) <= 0 {
                    print_err = Some("Printer failed to start page.".into());
                    break 'outer;
                }
                let _ = SetStretchBltMode(hdc, HALFTONE);
                let scanlines = StretchDIBits(
                    hdc,
                    dst_x,
                    dst_y,
                    dst_w,
                    dst_h,
                    0,
                    0,
                    w,
                    h,
                    Some(bgra.as_ptr().cast()),
                    &bmi,
                    DIB_RGB_COLORS,
                    SRCCOPY,
                );
                let end_ok = EndPage(hdc);
                if scanlines <= 0 || end_ok <= 0 {
                    print_err = Some("Printer failed while rendering page.".into());
                    break 'outer;
                }
            }
            pages_printed += 1;
        }
    }

    // SAFETY: Finalize the print document and release the printer DC.
    unsafe {
        let _ = EndDoc(hdc);
        let _ = DeleteDC(hdc);
    }
    free_globals(&pd);

    if let Some(err) = print_err {
        PrintOutcome::Error(err)
    } else {
        PrintOutcome::Printed(format!(
            "Printed {pages_printed} page{} via Windows Print.",
            if pages_printed == 1 { "" } else { "s" }
        ))
    }
}
