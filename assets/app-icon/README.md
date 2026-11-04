# RulZ Photocraft app icon

The **RulZ "R" mark** on a near-black tile with a red border: the same tile as the RulZ AI Tools
Suite's launcher (`tools/make_logo.py` in RulZ-AI-Tools-Suite), rendered larger.

## Palette

| Colour | Hex | Used for |
|---|---|---|
| RulZ red | `#ed000d` | the upper stroke and the border |
| Near-white | `#f2f2f2` | the lower stroke |
| Near-black | `#0a0a0c` | the tile |

## Geometry

A 512-unit tile (`viewBox="0 0 512 512"`), a rounded square with a red border. The macOS
renders pad it onto Apple's 824/1024 icon grid; the Windows and Linux renders use the whole tile,
as the other Crafting Apps do.

## Provenance

The RulZ logo, owned by speedyrulz. See `LICENSE.txt`.

## Files

- `photocraft.svg`: canonical master, a 1024 px PNG of the tile in an SVG wrapper (the logo has no vector source).
- `photocraft-small.svg`: the same at 256 px; also the hicolor scalable icon.
- `photocraft-1024.png`: 1024 px render on the macOS grid.
- `photocraft.icns`: macOS bundle icon (`CFBundleIconFile`).
- `photocraft.ico`: Windows icon, 16–256 px, embedded in the `.exe` by `apps/photocraft/build.rs`.
- `hicolor/<size>/apps/ai.storyteller.photocraft.png` (16–512) and `hicolor/scalable/...svg`:
  Linux icon theme.

The app also sets the window icon and Wayland app ID at runtime (`apps/photocraft/src/app_icon.rs`,
`main.rs`). On Windows the taskbar shows the icon of the Start Menu shortcut that launches the
exe, so the MSI's `<Icon Id>` keeps the `.exe` extension (ICE50); `app_icon.rs` documents the
details and tests the `.ico` sizes and the WiX icon references.

## Regenerate

Replace `photocraft.svg` (and `photocraft-small.svg`), then run `packaging/icons.sh`. It needs
`resvg`, plus `iconutil` on macOS; the `.ico` is packed by `cargo xtask ico`.
