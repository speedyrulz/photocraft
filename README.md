<p align="center">
  <img src="assets/app-icon/hicolor/256x256/apps/ai.storyteller.photocraft.png" width="128" height="128" alt="RulZ Photocraft icon: the RulZ R on a black tile with a red border">
</p>

<h1 align="center">RulZ Photocraft</h1>

<p align="center">
  <b>A native, open-source image editor in the style of Photoshop, written entirely in Rust.</b><br>
  <sub>Based on <a href="https://github.com/storytold/photocraft">PhotoCraft</a> by the ArtCraft team.</sub>
</p>

<p align="center">
  <img alt="100% Rust" src="https://img.shields.io/badge/100%25-Rust-b7410e?style=flat-square&logo=rust">
  <img alt="License: MIT OR Apache-2.0" src="https://img.shields.io/badge/license-MIT%20%2F%20Apache--2.0-3a3a3a?style=flat-square">
  <img alt="Status: early alpha" src="https://img.shields.io/badge/status-early%20alpha-d69e2e?style=flat-square">
</p>

<p align="center">
  <a href="https://github.com/speedyrulz/rulz-photocraft/wiki"><b>Wiki</b></a> ·
  <a href="https://github.com/speedyrulz/rulz-photocraft/wiki/Getting-Started">Getting Started</a> ·
  <a href="https://github.com/speedyrulz/rulz-photocraft/wiki/Features">Features</a> ·
  <a href="https://github.com/speedyrulz/rulz-photocraft/wiki/Plug-ins">Plug-ins</a> ·
  <a href="https://github.com/speedyrulz/rulz-photocraft/wiki/Automation">Automation</a>
</p>

<p align="center">
  <img src="docs/images/photocraft-demo.jpg" alt="The app editing Hokusai's The Great Wave: a caption card with a drop shadow, Title and Credit type layers, Vibrance and Curves adjustment layers, and the Curves editor drawn over the image's histogram" width="100%">
  <br>
  <sub><i>The Great Wave off Kanagawa</i>, Katsushika Hokusai, c. 1831</sub>
</p>

> [!IMPORTANT]
> RulZ Photocraft is in **early alpha**. Much of Photoshop's feature set exists in some form, but it isn't ready to replace Photoshop for daily professional work. The [roadmap](ROADMAP.md) and [known gaps](docs/gaps.md) show where it stands.

## What it does

- **Familiar:** menus, shortcuts, panels and tools follow Photoshop.
- **Non-destructive:** adjustment layers, masks, layer styles, smart objects and smart filters.
- **Real PSD files:** open, edit and save layered Photoshop documents, along with PSB, TIFF, PNG, JPEG, WebP and [more](https://github.com/speedyrulz/rulz-photocraft/wiki/File-Formats).
- **Fast and offline:** a native app with a GPU compositor. No Electron, web view, cloud or account.
- **Serious color:** RGB, Grayscale, CMYK and Lab at 8, 16 and 32 bits, with ICC color management.
- **Plug-ins:** **Plugins › Browse Plugins…** installs sandboxed WebAssembly filters from a checksum-pinned catalog of 50.
- **RulZ themes:** RulZ Dark and RulZ Light, alongside 14 other themes.
- **Scriptable:** every action is a command you can run from the [CLI, a JSON control channel or an MCP server](https://github.com/speedyrulz/rulz-photocraft/wiki/Automation).

<details>
<summary>See every tool</summary>

<h4>🧰 68 tools</h4>
      Move · Artboard · Rectangular and Elliptical Marquee · Single Row Marquee · Single Column Marquee · Lasso · Polygonal Lasso · Magnetic Lasso · Object Selection · Quick Selection · Magic Wand · Crop · Perspective Crop · Slice · Slice Select · Frame · Eyedropper · Color Sampler · Ruler · Note · Count · Remove · Spot Healing Brush · Healing Brush · Patch · Content-Aware Move · Red Eye · Brush · Pencil · Mixer Brush · Color Replacement · Clone Stamp · Pattern Stamp · History Brush · Art History Brush · Eraser · Background Eraser · Magic Eraser · Gradient · Paint Bucket · Blur · Sharpen · Smudge · Dodge · Burn · Sponge · Pen · Freeform Pen · Curvature Pen · Add Anchor Point · Delete Anchor Point · Convert Point · Horizontal and Vertical Type · Horizontal and Vertical Type Mask · Path Selection · Direct Selection · Rectangle · Ellipse · Triangle · Polygon · Line · Custom Shape · Hand · Rotate View · Zoom

</details>

## Get started

There are no prebuilt downloads yet, so for now you build it from source. You need [Rust](https://rustup.rs) 1.95 or newer, plus a few system packages on [Windows and Linux](https://github.com/speedyrulz/rulz-photocraft/wiki/Getting-Started).

```sh
git clone https://github.com/speedyrulz/rulz-photocraft
cd rulz-photocraft
cargo run --release -p photocraft
```

The first build takes a while. To open a file straight away, add its path: `cargo run --release -p photocraft -- image.psd`.

## Documentation

- **[Wiki](https://github.com/speedyrulz/rulz-photocraft/wiki):** guides for using the app.
- **[`docs/`](docs/) and the [documentation book](book/):** architecture, formats, automation and security.
- **[`AGENTS.md`](AGENTS.md):** start here if you're a contributor or an AI agent.
- **[`SECURITY.md`](SECURITY.md):** how to report a vulnerability.

## License and credits

Licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option. Copyright (c) 2026 ArtCraft Team and the PhotoCraft contributors. Required notices are in [NOTICE](NOTICE).

The RulZ name and logo are not covered by this license; see [`assets/rulz/LOGO-NOTICE.md`](assets/rulz/LOGO-NOTICE.md). Bundled assets keep their own licenses, listed in [ATTRIBUTION.md](ATTRIBUTION.md). Artwork shown is in the public domain; see [`docs/images/SOURCES.md`](docs/images/SOURCES.md).

<sub>Adobe and Photoshop are trademarks or registered trademarks of Adobe Inc. in the United States and/or other countries. This project is independent and open source, and is not affiliated with, sponsored by or endorsed by Adobe Inc.; these names are used only to describe the workflows it is compatible with.</sub>
