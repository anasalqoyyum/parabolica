#!/usr/bin/env python3
"""Regenerate portable review documents from the saved Doop HTML exports."""

from pathlib import Path
import json
import re


root = Path(__file__).resolve().parents[1]
design = root / "docs" / "design"
catalog = json.loads((design / "assets" / "catalog.json").read_text())
frames = json.loads((design / "frames.json").read_text())

for theme, screens in frames.items():
    for screen in screens:
        name = screen + ("-dark" if theme == "dark" else "")
        source = (design / f"{name}.html").read_text()
        for asset in catalog:
            source = source.replace(asset["url"], asset["asset"])
        (design / f"{name}-local.html").write_text(source)

gallery = (design / "gallery.html").read_text()
gallery = re.sub(
    r'/home/anas/Pictures/Screenshots/([^"<>]+)',
    r'screenshots/\1',
    gallery,
)
defaults = """<style>:root{--background:#fff;--foreground:#24272b;
--secondary:#f0f1f2;--border:#dddfe2;--primary:#315897;
--primary-foreground:#fff;--ring:#315897;--radius:6px;
--font-sans:'Segoe UI',Arial,sans-serif}body{background:var(--background)}
@media(prefers-color-scheme:dark){:root{--background:#17191c;
--foreground:#eceef1;--secondary:#282c32;--border:#3b424d;
--primary:#adcaff;--primary-foreground:#1b2028;--ring:#adcaff}}</style>"""
gallery = gallery.replace("</head>", defaults + "</head>")
(design / "index.html").write_text(gallery)

for source in re.findall(r'<img[^>]+src="([^"]+)"', gallery):
    if not source.startswith(("http:", "https:", "data:")):
        if not (design / source).is_file():
            raise SystemExit(f"Missing gallery image: {source}")

print("Regenerated portable design snapshots and index.html")
