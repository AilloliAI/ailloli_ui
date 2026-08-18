#!/usr/bin/env python3
"""Rebuild the deterministic, license-audited Ailloli UI file-icon font.

Requires Python 3 and fonttools==4.59.1. The input must be the unmodified
SymbolsNerdFontMono-Regular.ttf from the Nerd Fonts v3.4.0 Symbols Only
release. See SOURCE_PROVENANCE and THIRD_PARTY_NOTICES in this crate.
"""

from __future__ import annotations

import argparse
import hashlib
from pathlib import Path

from fontTools import __version__ as fonttools_version
from fontTools import subset
from fontTools.ttLib import TTFont


SOURCE_SHA256 = "f0f624d9b474bea1662cf7e862d44aebe1ae1f6c7f9cb7a0ca5d0e5ac9561c60"
OUTPUT_SHA256 = "2ad7a644bf07f6f752fbab646b96b599bdee6118800b3e17ffc132cea274431b"
FONTTOOLS_VERSION = "4.59.1"
FONT_LOGOS_RANGE = range(0xF300, 0xF382)
EXPLICIT_FALLBACKS = {0xF07B, 0xF15B}

# Unique glyphs returned by devicons 0.6.12's default (dark) map. Keep this
# list pinned: a devicons upgrade must be audited before regenerating the font.
DEVICONS_0_6_12_CODEPOINTS = {
    0x3E, 0x3BB, 0x221E, 0x235D, 0x2389, 0x25B2, 0x2699, 0xE21C,
    0xE271, 0xE28B, 0xE28C, 0xE600, 0xE603, 0xE606, 0xE607, 0xE608,
    0xE609, 0xE60A, 0xE60B, 0xE60C, 0xE60D, 0xE60E, 0xE60F, 0xE610,
    0xE611, 0xE614, 0xE615, 0xE619, 0xE61B, 0xE61C, 0xE61D, 0xE61E,
    0xE61F, 0xE620, 0xE623, 0xE624, 0xE625, 0xE627, 0xE628, 0xE62B,
    0xE62C, 0xE62D, 0xE62F, 0xE631, 0xE632, 0xE633, 0xE634, 0xE639,
    0xE63A, 0xE63B, 0xE64A, 0xE64B, 0xE652, 0xE655, 0xE656, 0xE65F,
    0xE660, 0xE666, 0xE667, 0xE670, 0xE672, 0xE674, 0xE677, 0xE67A,
    0xE682, 0xE684, 0xE688, 0xE68B, 0xE691, 0xE697, 0xE69A, 0xE69B,
    0xE69D, 0xE69E, 0xE69F, 0xE6A0, 0xE6A1, 0xE6A9, 0xE6AC, 0xE6AF,
    0xE6B2, 0xE6B3, 0xE6B4, 0xE702, 0xE706, 0xE707, 0xE70C, 0xE70E,
    0xE718, 0xE71E, 0xE728, 0xE736, 0xE737, 0xE738, 0xE745, 0xE749,
    0xE755, 0xE768, 0xE769, 0xE76A, 0xE772, 0xE775, 0xE779, 0xE786,
    0xE791, 0xE795, 0xE798, 0xE7A1, 0xE7A7, 0xE7A8, 0xE7A9, 0xE7AA,
    0xE7AF, 0xE7B1, 0xE7B4, 0xE7B8, 0xE7BA, 0xEAC4, 0xEAE8, 0xEAEB,
    0xEB9C, 0xEBC8, 0xEBE8, 0xF001, 0xF005, 0xF019, 0xF031, 0xF06D,
    0xF073, 0xF076, 0xF0AD, 0xF0C6, 0xF0EC, 0xF0FD, 0xF108, 0xF129,
    0xF15B, 0xF179, 0xF17C, 0xF1AB, 0xF1B2, 0xF20E, 0xF23E, 0xF296,
    0xF2B8, 0xF2D0, 0xF2F7, 0xF303, 0xF313, 0xF336, 0xF338, 0xF33C,
    0xF33D, 0xF34B, 0xF34C, 0xF34E, 0xF351, 0xF355, 0xF359, 0xF35A,
    0xF35B, 0xF35E, 0xF361, 0xF362, 0xF363, 0xF364, 0xF367, 0xF369,
    0xF36E, 0xF370, 0xF373, 0xF375, 0xF410, 0xF462, 0xF487, 0xF489,
    0xF48A, 0xF499, 0xF49B, 0xF4AE, 0xF006F, 0xF00AB, 0xF019A, 0xF01A7,
    0xF0219, 0xF021B, 0xF0227, 0xF022C, 0xF02A2, 0xF031B, 0xF032A, 0xF0331,
    0xF035B, 0xF042B, 0xF0483, 0xF04D9, 0xF0509, 0xF0565, 0xF057C, 0xF05C0,
    0xF05C6, 0xF05CA, 0xF0627, 0xF0673, 0xF06A9, 0xF06D3, 0xF0718, 0xF0721,
    0xF072B, 0xF07D4, 0xF0858, 0xF0868, 0xF08C7, 0xF099D, 0xF0A0A, 0xF0A16,
    0xF0AAE, 0xF0BC4, 0xF0CB9, 0xF0DD6, 0xF0EBE, 0xF0EEB, 0xF1049, 0xF1106,
    0xF121A, 0xF125F, 0xF13FF, 0xF1997, 0xF1998,
}

# Every included codepoint must belong to one of these source collections,
# whose redistribution terms are recorded in THIRD_PARTY_NOTICES.
LICENSED_RANGES = (
    range(0xE200, 0xE2AA),   # Font Awesome Extension, MIT
    range(0xE5FA, 0xE6B8),   # Seti-UI + Nerd Fonts custom, MIT
    range(0xE700, 0xE8F0),   # Devicons, MIT
    range(0xEA60, 0xEC1F),   # Codicons, CC-BY-4.0
    range(0xF000, 0xF300),   # Font Awesome icons, CC-BY-4.0 / font OFL-1.1
    range(0xF400, 0xF534),   # Octicons, MIT
    range(0xF0001, 0xF1AF1), # Material Design Icons, Apache-2.0
)


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def is_licensed(codepoint: int) -> bool:
    return any(codepoint in allowed for allowed in LICENSED_RANGES)


def audited_codepoints(source_font: TTFont) -> list[int]:
    source_cmap = source_font.getBestCmap() or {}
    candidates = DEVICONS_0_6_12_CODEPOINTS | EXPLICIT_FALLBACKS
    included: set[int] = set()

    for codepoint in sorted(candidates):
        if codepoint in FONT_LOGOS_RANGE:
            continue
        if codepoint not in source_cmap:
            continue
        if not is_licensed(codepoint):
            raise RuntimeError(
                f"unclassified glyph U+{codepoint:04X}; audit its source before inclusion"
            )
        included.add(codepoint)

    excluded = candidates - included
    unexpected = {
        codepoint
        for codepoint in excluded
        if codepoint in source_cmap and codepoint not in FONT_LOGOS_RANGE
    }
    if unexpected:
        formatted = ", ".join(f"U+{codepoint:04X}" for codepoint in sorted(unexpected))
        raise RuntimeError(f"unexpected excluded source glyphs: {formatted}")

    if len(included) != 198:
        raise RuntimeError(f"expected 198 audited glyphs, got {len(included)}")
    return sorted(included)


def set_name(font: TTFont, name_id: int, value: str) -> None:
    names = font["name"]
    names.setName(value, name_id, 1, 0, 0)
    names.setName(value, name_id, 3, 1, 0x409)


def rename_subset(font: TTFont) -> None:
    names = font["name"]
    retained_ids = {0, 1, 2, 3, 4, 5, 6, 13}
    names.names = [record for record in names.names if record.nameID not in retained_ids]
    set_name(
        font,
        0,
        "Copyright (c) 2016 Ryan McIntyre; subset modifications Copyright 2026 "
        "Rising Corporation and Ailloli UI contributors. See THIRD_PARTY_NOTICES.",
    )
    set_name(font, 1, "Ailloli UI File Icons")
    set_name(font, 2, "Regular")
    set_name(font, 3, "Ailloli UI File Icons 1.0")
    set_name(font, 4, "Ailloli UI File Icons")
    set_name(font, 5, "Version 1.000; subset of Nerd Fonts 3.4.0")
    set_name(font, 6, "AilloliUIFileIcons")
    set_name(
        font,
        13,
        "Composite subset; MIT, CC-BY-4.0, Apache-2.0 and OFL-1.1 terms apply. "
        "See THIRD_PARTY_NOTICES.",
    )


def rebuild(source: Path, output: Path) -> None:
    if fonttools_version != FONTTOOLS_VERSION:
        raise RuntimeError(
            f"fonttools version mismatch: expected {FONTTOOLS_VERSION}, got {fonttools_version}"
        )

    actual_source_hash = sha256(source)
    if actual_source_hash != SOURCE_SHA256:
        raise RuntimeError(
            f"source SHA-256 mismatch: expected {SOURCE_SHA256}, got {actual_source_hash}"
        )

    font = TTFont(source, recalcTimestamp=False)
    codepoints = audited_codepoints(font)

    options = subset.Options()
    options.drop_tables.append("PfEd")
    options.layout_features = []
    options.name_IDs = ["*"]
    options.name_languages = ["*"]
    options.name_legacy = True
    options.notdef_glyph = True
    options.notdef_outline = True
    options.recalc_timestamp = False
    options.canonical_order = True

    subsetter = subset.Subsetter(options=options)
    subsetter.populate(unicodes=codepoints)
    subsetter.subset(font)
    rename_subset(font)
    font.save(output, reorderTables=True)

    rebuilt = TTFont(output, recalcTimestamp=False)
    rebuilt_cmap = rebuilt.getBestCmap() or {}
    rebuilt_codepoints = set(rebuilt_cmap.keys())
    if rebuilt_codepoints != set(codepoints):
        raise RuntimeError("rebuilt cmap differs from the audited codepoint set")
    if any(codepoint in rebuilt_codepoints for codepoint in FONT_LOGOS_RANGE):
        raise RuntimeError("rebuilt font contains an unlicensed Font Logos glyph")
    unmapped_glyphs = set(rebuilt.getGlyphOrder()) - set(rebuilt_cmap.values()) - {".notdef"}
    if unmapped_glyphs:
        raise RuntimeError(f"rebuilt font contains unmapped glyphs: {sorted(unmapped_glyphs)}")

    actual_output_hash = sha256(output)
    if actual_output_hash != OUTPUT_SHA256:
        raise RuntimeError(
            f"output SHA-256 mismatch: expected {OUTPUT_SHA256}, got {actual_output_hash}"
        )

    print(f"fonttools={FONTTOOLS_VERSION}")
    print(f"source_sha256={actual_source_hash}")
    print(f"output_sha256={actual_output_hash}")
    print(f"included_codepoints={len(codepoints)}")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--source", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    rebuild(args.source, args.output)


if __name__ == "__main__":
    main()
