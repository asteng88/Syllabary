import io
import os
import struct
import numpy as np
from PIL import Image, ImageDraw, ImageFilter, ImageFont

ASSETS_DIR = r"p:\Syllabary\app\assets\icons"
LINUX_DIR = os.path.join(ASSETS_DIR, "linux")
ARTIFACT_DIR = r"C:\Users\asten\.gemini\antigravity\brain\03bf48c7-ed61-49f3-b655-ade7f0310555"
FONTS_DIR = r"p:\Syllabary\app\fonts"

os.makedirs(ASSETS_DIR, exist_ok=True)
os.makedirs(LINUX_DIR, exist_ok=True)

S = 2048  # 2x supersampled master canvas -> 1024x1024


def squircle_mask(size: int, margin: float = 0.045, power: float = 4.6) -> np.ndarray:
    y, x = np.mgrid[0:size, 0:size].astype(np.float32)
    c = (size - 1) * 0.5
    r = size * (0.5 - margin)
    nx = np.abs((x - c) / r)
    ny = np.abs((y - c) / r)
    d = (nx ** power + ny ** power) ** (1.0 / power)
    edge = 1.5 / r
    return np.clip((1.0 - d) / edge + 0.5, 0.0, 1.0)


def make_gradient_bg(size: int, c_top, c_bot, radial_glow=None) -> Image.Image:
    y = np.linspace(0.0, 1.0, size, dtype=np.float32)[:, None]
    x = np.linspace(0.0, 1.0, size, dtype=np.float32)[None, :]
    t = np.clip(y * 0.85 + x * 0.15, 0.0, 1.0)
    rgb = np.zeros((size, size, 3), dtype=np.float32)
    for ch in range(3):
        rgb[:, :, ch] = c_top[ch] * (1.0 - t) + c_bot[ch] * t
    if radial_glow is not None:
        gx, gy, gr, gcol, gstr = radial_glow
        dist2 = ((x - gx) ** 2 + (y - gy) ** 2) / (gr ** 2)
        glow = np.exp(-dist2 * 1.8) * gstr
        for ch in range(3):
            rgb[:, :, ch] = rgb[:, :, ch] * (1.0 - glow) + gcol[ch] * glow
    rgb = np.clip(rgb, 0, 255).astype(np.uint8)
    alpha = np.full((size, size, 1), 255, dtype=np.uint8)
    return Image.fromarray(np.concatenate([rgb, alpha], axis=2), "RGBA")


def apply_squircle_and_rim(img: Image.Image, margin=0.045, rim_color=(255, 255, 255), rim_alpha=75) -> Image.Image:
    size = img.size[0]
    outer = squircle_mask(size, margin=margin, power=4.6)
    inner = squircle_mask(size, margin=margin + 0.0055, power=4.6)
    rim = np.clip(outer - inner, 0.0, 1.0)
    y = np.linspace(0.0, 1.0, size, dtype=np.float32)[:, None]
    top_weight = np.clip(1.25 - y * 1.15, 0.2, 1.0)

    arr = np.array(img, dtype=np.float32)
    ra = (rim_alpha / 255.0) * rim * top_weight
    for ch in range(3):
        arr[:, :, ch] = arr[:, :, ch] * (1.0 - ra) + rim_color[ch] * ra
    arr[:, :, 3] = np.clip(outer * 255.0, 0, 255)
    return Image.fromarray(np.clip(arr, 0, 255).astype(np.uint8), "RGBA")


def draw_soft_shadow(canvas: Image.Image, shape_fn, offset=(0, 28), blur=36, opacity=95):
    sh = Image.new("RGBA", canvas.size, (0, 0, 0, 0))
    d = ImageDraw.Draw(sh)
    shape_fn(d, (10, 16, 30, opacity))
    sh = sh.filter(ImageFilter.GaussianBlur(blur))
    shifted = Image.new("RGBA", canvas.size, (0, 0, 0, 0))
    shifted.paste(sh, offset)
    return Image.alpha_composite(canvas, shifted)


def render_app_icon_b() -> Image.Image:
    """Concept B: Obsidian Serif S — Main Syllabary App Icon."""
    bg = make_gradient_bg(
        S,
        (42, 42, 48),
        (17, 17, 21),
        radial_glow=(0.5, 0.22, 0.58, (90, 90, 112), 0.26),
    )

    pill = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    px0, py0, px1, py1 = int(S * 0.19), int(S * 0.43), int(S * 0.81), int(S * 0.58)
    pw, ph = px1 - px0, py1 - py0
    gx = np.linspace(0.0, 1.0, pw, dtype=np.float32)[None, :]
    c1 = np.array([245, 158, 11], dtype=np.float32)
    c2 = np.array([236, 72, 153], dtype=np.float32)
    strip = np.zeros((ph, pw, 4), dtype=np.uint8)
    for ch in range(3):
        strip[:, :, ch] = np.clip(c1[ch] * (1.0 - gx) + c2[ch] * gx, 0, 255).astype(np.uint8)
    strip[:, :, 3] = 235
    strip_img = Image.fromarray(strip, "RGBA")
    pmask = Image.new("L", (pw, ph), 0)
    ImageDraw.Draw(pmask).rounded_rectangle([0, 0, pw - 1, ph - 1], radius=ph // 2, fill=255)
    pill.paste(strip_img, (px0, py0), pmask)

    glow = pill.filter(ImageFilter.GaussianBlur(42))
    garr = np.array(glow, dtype=np.float32)
    garr[:, :, 3] *= 0.45
    bg = Image.alpha_composite(bg, Image.fromarray(garr.astype(np.uint8), "RGBA"))
    bg = Image.alpha_composite(bg, pill)

    font_path = os.path.join(FONTS_DIR, "playfair-display", "bold.ttf")
    font = ImageFont.truetype(font_path, int(S * 0.67))
    tmp = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    td = ImageDraw.Draw(tmp)
    bbox = td.textbbox((0, 0), "S", font=font)
    tw, th = bbox[2] - bbox[0], bbox[3] - bbox[1]
    tx = (S - tw) // 2 - bbox[0]
    ty = (S - th) // 2 - bbox[1] - int(S * 0.015)

    sh = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    ImageDraw.Draw(sh).text((tx, ty + 28), "S", font=font, fill=(0, 0, 0, 145))
    sh = sh.filter(ImageFilter.GaussianBlur(30))
    bg = Image.alpha_composite(bg, sh)

    ImageDraw.Draw(tmp).text((tx, ty), "S", font=font, fill=(252, 249, 242, 255))
    bg = Image.alpha_composite(bg, tmp)

    out = apply_squircle_and_rim(bg, rim_color=(255, 255, 255), rim_alpha=70)
    return out.resize((1024, 1024), Image.Resampling.LANCZOS)


def make_cobalt_nib_base() -> Image.Image:
    """Shared Concept A base layer (Cobalt squircle + elevated folded ivory sheet + gold nib)."""
    bg = make_gradient_bg(
        S,
        (37, 99, 235),
        (23, 37, 84),
        radial_glow=(0.28, 0.22, 0.65, (147, 197, 253), 0.28),
    )

    px0, py0 = int(S * 0.21), int(S * 0.15)
    px1, py1 = int(S * 0.75), int(S * 0.83)
    fold = int(S * 0.135)
    rad = int(S * 0.036)

    def page_poly(d, col):
        pts = [
            (px0 + rad, py0),
            (px1 - fold, py0),
            (px1, py0 + fold),
            (px1, py1 - rad),
            (px1 - rad, py1),
            (px0 + rad, py1),
            (px0, py1 - rad),
            (px0, py0 + rad),
        ]
        d.polygon(pts, fill=col)
        d.ellipse([px0, py0, px0 + 2 * rad, py0 + 2 * rad], fill=col)
        d.ellipse([px0, py1 - 2 * rad, px0 + 2 * rad, py1], fill=col)
        d.ellipse([px1 - 2 * rad, py1 - 2 * rad, px1, py1], fill=col)

    bg = draw_soft_shadow(bg, page_poly, offset=(0, 32), blur=44, opacity=115)

    sheet = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    sd = ImageDraw.Draw(sheet)
    page_poly(sd, (250, 248, 243, 255))

    # Folded corner
    sd.polygon(
        [(px1 - fold, py0), (px1, py0 + fold), (px1 - fold - 14, py0 + fold + 20)],
        fill=(180, 195, 220, 95),
    )
    sd.polygon(
        [(px1 - fold, py0), (px1, py0 + fold), (px1 - fold, py0 + fold)],
        fill=(222, 232, 248, 255),
    )

    # Clean typographic lines below the top-left format label
    lx0 = px0 + int(S * 0.065)
    line_specs = [
        (0.41, 0.37, (148, 163, 184, 200), 0.019),
        (0.475, 0.37, (148, 163, 184, 200), 0.019),
        (0.54, 0.27, (148, 163, 184, 200), 0.019),
        (0.625, 0.30, (148, 163, 184, 165), 0.019),
        (0.69, 0.21, (148, 163, 184, 165), 0.019),
    ]
    for ly_f, lw_f, col, lh_f in line_specs:
        ly = int(S * ly_f)
        lh = int(S * lh_f)
        lw = int(S * lw_f)
        sd.rounded_rectangle([lx0, ly, lx0 + lw, ly + lh], radius=lh // 2, fill=col)

    bg = Image.alpha_composite(bg, sheet)

    # Metallic gold fountain pen nib in lower-right
    nib_layer = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    ns = 860
    nimg = Image.new("RGBA", (ns, ns), (0, 0, 0, 0))
    n_d = ImageDraw.Draw(nimg)
    cx = ns // 2
    n_d.rounded_rectangle([cx - 120, 620, cx + 120, 725], radius=22, fill=(180, 128, 42, 255))
    n_d.rounded_rectangle([cx - 105, 632, cx + 105, 713], radius=16, fill=(245, 208, 118, 255))
    n_d.polygon([(cx, 105), (cx - 168, 410), (cx - 108, 625), (cx, 625)], fill=(252, 222, 138, 255))
    n_d.polygon([(cx, 105), (cx + 168, 410), (cx + 108, 625), (cx, 625)], fill=(217, 163, 58, 255))
    n_d.line([(cx, 125), (cx, 420)], fill=(30, 41, 74, 255), width=15)
    n_d.ellipse([cx - 32, 396, cx + 32, 460], fill=(30, 41, 74, 255))
    n_d.ellipse([cx - 20, 408, cx + 20, 448], fill=(52, 76, 130, 255))

    rot = nimg.rotate(-36, resample=Image.Resampling.BICUBIC, expand=True)
    nsh = Image.new("RGBA", rot.size, (0, 0, 0, 0))
    nsh.paste((15, 23, 42, 115), (0, 0), rot.split()[3])
    nsh = nsh.filter(ImageFilter.GaussianBlur(26))

    pos_x = int(S * 0.47)
    pos_y = int(S * 0.42)
    nib_layer.paste(nsh, (pos_x + 12, pos_y + 24), nsh)
    nib_layer.paste(rot, (pos_x, pos_y), rot)
    bg = Image.alpha_composite(bg, nib_layer)
    return bg


def render_doc_icon(base: Image.Image, label: str, accent_rgb: tuple) -> Image.Image:
    """Renders Concept A with bold document type text on the top-left of the ivory sheet."""
    img = base.copy()
    overlay = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    d = ImageDraw.Draw(overlay)

    # Top-left of the ivory sheet (px0 = 0.21*S, py0 = 0.15*S; fold starts at 0.615*S)
    lx = int(S * 0.268)
    ly = int(S * 0.215)

    # Bold high-legibility sans font (Inter Bold) sized cleanly to fit top-left of sheet
    font_size = int(S * (0.118 if len(label) <= 3 else 0.104))
    font_path = os.path.join(FONTS_DIR, "inter", "bold.ttf")
    font = ImageFont.truetype(font_path, font_size)

    # Accent underline/rule right beneath the bold type label
    bbox = d.textbbox((lx, ly), label, font=font)
    # Draw crisp bold label directly on top-left of the ivory sheet
    d.text((lx, ly), label, font=font, fill=(accent_rgb[0], accent_rgb[1], accent_rgb[2], 255))
    rule_y = bbox[3] + int(S * 0.022)
    rule_w = max(bbox[2] - bbox[0], int(S * 0.18))
    rule_h = int(S * 0.018)
    d.rounded_rectangle(
        [lx, rule_y, lx + rule_w, rule_y + rule_h],
        radius=rule_h // 2,
        fill=(accent_rgb[0], accent_rgb[1], accent_rgb[2], 225),
    )

    img = Image.alpha_composite(img, overlay)
    out = apply_squircle_and_rim(img, rim_color=(219, 234, 254), rim_alpha=95)
    return out.resize((1024, 1024), Image.Resampling.LANCZOS)


def save_ico(img: Image.Image, path: str):
    """Saves a multi-resolution Windows .ico (16, 24, 32, 48, 64, 128, 256px)."""
    sizes = [(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)]
    img.save(path, format="ICO", sizes=sizes)


def save_icns(img: Image.Image, path: str):
    """Writes a standard macOS .icns container with PNG payloads (16px through 1024px)."""
    # Apple ICNS OSTypes that accept PNG data directly (supported since macOS 10.7+)
    entries = [
        (b"icp4", 16),
        (b"icp5", 32),
        (b"icp6", 64),
        (b"ic07", 128),
        (b"ic08", 256),
        (b"ic09", 512),
        (b"ic10", 1024),
    ]
    chunks = bytearray()
    for ostype, sz in entries:
        resized = img if sz == img.size[0] else img.resize((sz, sz), Image.Resampling.LANCZOS)
        buf = io.BytesIO()
        resized.save(buf, format="PNG", optimize=True)
        payload = buf.getvalue()
        chunks.extend(ostype)
        chunks.extend(struct.pack(">I", 8 + len(payload)))
        chunks.extend(payload)
    total_len = 8 + len(chunks)
    with open(path, "wb") as f:
        f.write(b"icns")
        f.write(struct.pack(">I", total_len))
        f.write(chunks)


def main():
    # 1. Main App Icon (Concept B)
    app_icon = render_app_icon_b()
    app_icon.save(os.path.join(ASSETS_DIR, "syllabary.png"))
    save_ico(app_icon, os.path.join(ASSETS_DIR, "syllabary.ico"))
    save_icns(app_icon, os.path.join(ASSETS_DIR, "syllabary.icns"))
    for sz in (16, 24, 32, 48, 64, 128, 256, 512, 1024):
        resized = app_icon if sz == 1024 else app_icon.resize((sz, sz), Image.Resampling.LANCZOS)
        resized.save(os.path.join(LINUX_DIR, f"syllabary_{sz}.png"))
    # Embedded runtime window icon (256x256 RGBA PNG for fast startup in main.rs)
    app_icon.resize((256, 256), Image.Resampling.LANCZOS).save(os.path.join(ASSETS_DIR, "syllabary_256.png"))

    # 2. Document Association Icons (Concept A + bold top-left file type text)
    doc_types = [
        ("wpx",  "WPX",  (30, 64, 175)),   # Syllabary Native (Royal Cobalt)
        ("docx", "DOCX", (29, 78, 216)),   # Word Document (Microsoft Blue)
        ("pdf",  "PDF",  (220, 38, 38)),   # PDF Document (Editorial Crimson)
        ("gdoc", "GDOC", (26, 115, 232)),  # Google Docs (Google Blue)
        ("md",   "MD",   (79, 70, 229)),   # Markdown (Indigo)
        ("rtf",  "RTF",  (13, 148, 136)),  # Rich Text (Teal)
        ("txt",  "TXT",  (71, 85, 105)),   # Plain Text (Slate)
    ]
    base_a = make_cobalt_nib_base()
    rendered_docs = []
    for ext, label, accent in doc_types:
        dimg = render_doc_icon(base_a, label, accent)
        dimg.save(os.path.join(ASSETS_DIR, f"doc_{ext}.png"))
        save_ico(dimg, os.path.join(ASSETS_DIR, f"doc_{ext}.ico"))
        save_icns(dimg, os.path.join(ASSETS_DIR, f"doc_{ext}.icns"))
        rendered_docs.append((label, dimg))

    # 3. Preview sheet of Main App Icon + all 7 Document Association Icons
    pw, ph = 1600, 620
    sheet = Image.new("RGBA", (pw, ph), (244, 245, 248, 255))
    d = ImageDraw.Draw(sheet)
    hfont = ImageFont.truetype(os.path.join(FONTS_DIR, "inter", "bold.ttf"), 22)
    sfont = ImageFont.truetype(os.path.join(FONTS_DIR, "inter", "regular.ttf"), 16)

    d.text((50, 28), "Main App Icon (Taskbar / Dock / EXE)", font=hfont, fill=(24, 24, 27, 255))
    big_app = app_icon.resize((210, 210), Image.Resampling.LANCZOS)
    sheet.paste(big_app, (75, 76), big_app)
    d.text((100, 300), "Syllabary App", font=hfont, fill=(24, 24, 27, 255))

    d.text((380, 28), "Document File-Association Icons (Concept A + Format Label)", font=hfont, fill=(24, 24, 27, 255))
    for i, (label, dimg) in enumerate(rendered_docs):
        col = i % 4
        row = i // 4
        x = 395 + col * 295
        y = 78 + row * 255
        sm = dimg.resize((175, 175), Image.Resampling.LANCZOS)
        sheet.paste(sm, (x, y), sm)
        tb = d.textbbox((0, 0), f".{label.lower()}", font=hfont)
        d.text((x + 87 - (tb[2] - tb[0]) // 2, y + 185), f".{label.lower()}", font=hfont, fill=(30, 41, 59, 255))

    sheet.save(os.path.join(ARTIFACT_DIR, "final_icon_suite_preview.png"))
    print("Generated full cross-platform icon suite in", ASSETS_DIR)


if __name__ == "__main__":
    main()
