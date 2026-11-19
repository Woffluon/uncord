# uncord — Brand & Logo Guidelines

**Hardware Port** pairs a `rx=48` studio chassis plate with a top-right negative-space `u` acoustic channel and a centered solid signal core.

## 1. Master Files

| File | Canvas | Use |
|---|---|---|
| `brand/uncord-symbol-color.svg` | `256 × 256` | Primary symbol on light surfaces (`#111214` + `#F25C2A`) |
| `brand/uncord-symbol-small.svg` | `256 × 256` | Widened-channel cut for 16–32 px tray, favicon, and dense UI |
| `brand/uncord-lockup-color.svg` | `960 × 256` | Horizontal lockup for light surfaces (README light, docs, web) |
| `brand/uncord-lockup-dark.svg` | `960 × 256` | Horizontal lockup for dark surfaces (Obsidian UI, README dark) |
| `brand/exports/` | Various | Monochrome (`black`, `white`, `mono-f25c2a`), square, app icon, web/PWA raster set |

All wordmark letterforms (`uncord`) are custom-constructed vector paths. No runtime font is required to render the lockups.

## 2. Colour Palette

| Role | Name | HEX | RGB | CMYK (approx.) | Pantone (approx.) |
|---|---|---|---|---|---|
| Primary Ink (Light UI) | Chassis Ink | `#111214` | `17, 18, 20` | `76 68 62 84` | `Black 6 C` |
| Reversed Ink (Dark UI) | Chalk Ink | `#F5F5F3` | `245, 245, 243` | `2 2 3 0` | `Cool Gray 1 C` |
| Signal Accent | Signal Vermilion | `#F25C2A` | `242, 92, 42` | `0 76 89 0` | `1655 C` |

**Signal Vermilion rule:** Apply `#F25C2A` **only** to the centered solid signal core inside the `u` acoustic channel (and active route indicators in the UI). Never recolour the outer hardware plate to Vermilion in full-colour marks, and never tint the `uncord` wordmark orange.

Verify physical Pantone and CMYK proofs before offset or screen-print runs.

## 3. Clear Space & Minimum Sizes

- **Clear space:** Maintain at least `1 × c` on all sides, where `c` equals the width of the `u` negative-space channel (`36 / 256 ≈ 14%` of symbol height). Keep text, borders, and controls outside this zone.
- **Minimum sizes:**
  - **Windows system tray:** `16 × 16 px` (`uncord-symbol-small.svg` / `src-tauri/icons/tray-icon.png`)
  - **Application / toolbar UI icon:** `24 × 24 px` (symbol only)
  - **Horizontal lockup:** `120 px` wide (`32 px` height) on screen; `30 mm` wide in print

## 4. Approved Backgrounds

- **Light surfaces (`#FFFFFF`, `#F5F5F3`):** Use `uncord-symbol-color.svg` or `uncord-lockup-color.svg` (`#111214` plate + `#F25C2A` core).
- **Dark surfaces (`#0D0E10`, `#111214`, `#15171A`):** Use `uncord-lockup-dark.svg` (`#F5F5F3` plate + `#F25C2A` core + `#F5F5F3` wordmark) or the white reversed export.
- **Tile / App Icon (`#111214` tile):** Place the `#F5F5F3` plate + `#F25C2A` core inside the `#111214` container (`brand/exports/uncord-symbol-app-icon.svg`).
- **One-colour / engraving / laser:** Use `uncord-symbol-black.svg`, `uncord-symbol-white.svg`, or `uncord-symbol-mono-f25c2a.svg`. The crescent collar preserves core separation in single-ink reproduction.

## 5. Usage Don'ts

- Do not stretch, skew, or rotate the symbol or lockup.
- Do not place `#111214` Chassis Ink directly on dark backgrounds without switching to `#F5F5F3` Chalk Ink.
- Do not add drop shadows, outer glows, gradients, or strokes to the master geometry.
- Do not retype the `uncord` wordmark in a stock font or alter the symbol-to-wordmark spacing.
- Do not compress the symbol below `16 × 16 px` or the horizontal lockup below `120 px` width.

## 6. Legal Note

Automated library comparisons do not substitute for legal trademark clearance. Run formal trademark and reverse-image searches in target jurisdictions prior to commercial trademark filing.
