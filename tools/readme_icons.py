#!/usr/bin/env python3
"""The README icons of Autumn Natter.

The icons of the program are one colour, and the window tints them as it likes.
That does not serve a README: the same page is read on the light theme of GitHub
and on the dark one, and a black glyph on a transparent plate vanishes on one of
them. So each icon is stood on a soft plate of its own and tinted with a colour
of autumn, which reads on both.

Nothing new is drawn. The shape of every icon is read from `assets/icons`, so a
change to an icon of the program shows here the next time this is run:

    python3 tools/readme_icons.py

The pictures are written to `assets/readme`, and README.md points at them.
"""

import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
SRC = ROOT / "assets" / "icons"
OUT = ROOT / "assets" / "readme"

# The colours of autumn, and the plate each one sits on. A colour is chosen for
# the work it stands for: ember for the chat itself, the frost of a snowflake
# for the colding of a long talk, and so on.
PALETTE = {
    "ember": {"glyph": "#C2410C", "plate": "#FFF3E8", "edge": "#F0D6BC"},
    "amber": {"glyph": "#A16207", "plate": "#FEF7E4", "edge": "#EDDCB2"},
    "olive": {"glyph": "#4D7C0F", "plate": "#F3F8EB", "edge": "#D9E6C2"},
    "bronze": {"glyph": "#7C4A21", "plate": "#F8F1EA", "edge": "#E4D3C2"},
    "plum": {"glyph": "#9D174D", "plate": "#FCF0F4", "edge": "#EFD2DD"},
    "slate": {"glyph": "#475569", "plate": "#F1F5F9", "edge": "#D6DEE8"},
    "frost": {"glyph": "#0F766E", "plate": "#EEF7F6", "edge": "#C9E2DF"},
    "berry": {"glyph": "#B91C1C", "plate": "#FDF0EF", "edge": "#F0D2CF"},
}

# The icons the README uses, with the colour they are tinted in.
ICONS = {
    "chat": "ember",
    "send": "amber",
    "plus": "olive",
    "model": "ember",
    "gpu": "slate",
    "settings": "amber",
    "think": "plum",
    "chevron-down": "plum",
    "terminal": "slate",
    "code": "frost",
    "shield": "olive",
    "snowflake": "frost",
    "tooltip": "bronze",
    "warning": "berry",
}

# The words a user of the picture is given, since a shape of lines says little
# on its own.
LABELS = {
    "chat": "Chats",
    "send": "Attachments",
    "plus": "A new chat",
    "model": "The model",
    "gpu": "Graphics cards",
    "settings": "Settings",
    "think": "Thinking blocks",
    "chevron-down": "Folds",
    "terminal": "The terminal tool",
    "code": "The file tools",
    "shield": "The sandbox",
    "snowflake": "Compaction",
    "tooltip": "Web search",
    "warning": "A fault, told plainly",
}

INNER = re.compile(r"<svg[^>]*>(.*)</svg>\s*$", re.S)

HEAD = (
    '<?xml version="1.0" encoding="UTF-8"?>\n'
    "<!--\n"
    "  Autumn Natter - an icon of the README.\n"
    "  The shape is the icon of the program in assets/icons/{name}; it is stood\n"
    "  on a plate and tinted, so that it reads on both the light theme of\n"
    "  GitHub and the dark one.\n"
    "  Made by tools/readme_icons.py - do not mend it by hand.\n"
    "-->\n"
)


def inner_of(path: pathlib.Path) -> str:
    text = path.read_text(encoding="utf-8")
    found = INNER.search(text)
    if not found:
        sys.exit("no svg root in " + str(path))
    return found.group(1).strip()


def chip(name: str, colour: str, size: int = 64) -> str:
    tint = PALETTE[colour]["glyph"]
    body = inner_of(SRC / (name + ".svg")).replace("#000", tint)
    body = "\n".join("  " + line for line in body.splitlines())
    pad = size * 0.15625          # the glyph holds 70 per cent of the plate
    scale = (size - 2 * pad) / 24.0
    return (
        HEAD.format(name=name + ".svg")
        + '<svg width="{size}" height="{size}" viewBox="0 0 {size} {size}" '
        'xmlns="http://www.w3.org/2000/svg" role="img" aria-label="{label}">\n'.format(
            size=size, label=LABELS.get(name, name)
        )
        + '  <rect x="2" y="2" width="{box}" height="{box}" rx="{rx}" fill="{plate}" '
        'stroke="{edge}" stroke-width="2"/>\n'.format(
            box=size - 4,
            rx=int(size * 0.3125),
            plate=PALETTE[colour]["plate"],
            edge=PALETTE[colour]["edge"],
        )
        # The root of an icon of the program says that a shape is not filled.
        # The plate takes that rule over, for a closed shape filled with the
        # tint is a blob and not a drawing.
        + '  <g transform="translate({pad} {pad}) scale({scale:.5f})" fill="none">\n'.format(
            pad=round(pad, 2), scale=scale
        )
        + body
        + "\n  </g>\n</svg>\n"
    )


def app(size: int = 128) -> str:
    """The icon of the program itself, on a plate of the same shape."""
    body = inner_of(SRC / "autumn-natter.svg")
    body = "\n".join("  " + line for line in body.splitlines())
    pad = size * 0.09375
    scale = (size - 2 * pad) / 128.0
    return (
        '<?xml version="1.0" encoding="UTF-8"?>\n'
        "<!--\n"
        "  Autumn Natter - the icon of the README.\n"
        "  The leaf of the program in assets/icons/autumn-natter.svg, on a plate\n"
        "  so that it reads on both the themes of GitHub.\n"
        "  Made by tools/readme_icons.py - do not mend it by hand.\n"
        "-->\n"
        + '<svg width="{size}" height="{size}" viewBox="0 0 {size} {size}" '
        'xmlns="http://www.w3.org/2000/svg" role="img" aria-label="Autumn Natter">\n'.format(
            size=size
        )
        + '  <rect x="2" y="2" width="{box}" height="{box}" rx="{rx}" fill="{plate}" '
        'stroke="{edge}" stroke-width="2"/>\n'.format(
            box=size - 4, rx=int(size * 0.234), plate="#FFF6EC", edge="#EFDCC8"
        )
        + '  <g transform="translate({pad} {pad}) scale({scale:.5f})">\n'.format(
            pad=round(pad, 2), scale=scale
        )
        + body
        + "\n  </g>\n</svg>\n"
    )


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    written = [OUT / "autumn-natter-plate.svg"]
    written[0].write_text(app(), encoding="utf-8")
    for name, colour in ICONS.items():
        path = OUT / (name + ".svg")
        path.write_text(chip(name, colour), encoding="utf-8")
        written.append(path)

    # The folder holds nothing but what is made here, so a picture left over
    # from an icon that is no longer used is taken away.
    for path in OUT.glob("*.svg"):
        if path not in written:
            path.unlink()
            print("gone  " + str(path.relative_to(ROOT)))
    for path in written:
        print(path.relative_to(ROOT))


if __name__ == "__main__":
    main()
