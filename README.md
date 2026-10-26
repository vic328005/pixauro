<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="editor/src/assets/pixauro-mark.svg">
    <img src="editor/src/assets/pixauro-mark-light.svg" width="54" height="54" alt="Pixauro">
  </picture>
</p>

# Pixauro

**A desktop editor for pixel art and sprite animations.**

English · [简体中文](README.zh-CN.md) · [日本語](README.ja.md)

Draw from scratch, or turn a PNG into a set of editable animation frames. Pixauro brings drawing, palettes, sprite slicing, and image pixelation into one workspace, with a CLI for processing assets from scripts.

Pixauro is in early development. Project files and APIs may change without backward compatibility. The editor interface is currently mostly in English; this README is available in three languages.

## What you can do

- **Draw and edit.** Brush, eraser, eyedropper, magic wand selection, and cel movement, with undo, redo, and a browsable history.
- **Work with palettes.** Edit RGBA colors shared across the project, extract colors from PNGs, and export a palette as a PNG. Changing a palette entry updates the pixels that use it.
- **Animate with layers and frames.** Keep multiple animations in one project, set their frame rates separately, preview playback with onion skinning, and link cels to share pixels across frames.
- **Prepare existing artwork.** Slice a PNG into frames, adjust crop regions and offsets, reduce resolution and color count, and preview background removal before importing. You can also keep the original cropped pixels.
- **Export assets.** Save a frame as PNG, or export animations as a sprite sheet with JSON metadata. The CLI can also produce looping GIF previews.
- **Arrange your workspace.** Move and resize panels, toggle the pixel grid, and choose a light or dark transparency backdrop.

Pixelation processes each frame separately. Background removal and automatic slice positioning can need manual correction; they do not identify characters or guarantee consistency between frames.

## Run from source

You will need:

- A current stable Rust toolchain with Cargo.
- Node.js and npm. The frontend accepts Node `^20.19.0 || ^22.13.0 || >=24`.
- Git LFS for binary assets, including application icons.
- The system dependencies listed in the [Tauri 2 prerequisites](https://v2.tauri.app/start/prerequisites/) for your OS.

After cloning the repository, run these commands from its root:

```sh
git lfs install --local
git lfs pull
npm --prefix editor ci
npm --prefix editor run desktop
```

The `desktop` command starts both the frontend development server and the native application. Opening the frontend in a regular browser does not provide the native file and editing commands.

To build a desktop package for your current platform:

```sh
npm --prefix editor run desktop:build
```

### Your first project

1. Create a project and choose a canvas size.
2. Pick a palette color and draw, or drop a PNG onto the canvas to open the importer.
3. Add frames in the timeline. For a sprite sheet import, each row becomes an animation; review the slices and playback before importing.
4. Save the project as JSON to keep its layers, animations, palette, and edit history. Use **Export** for PNG artwork or animation assets.

## Command line

The CLI uses the same asset pipeline as the editor and does not need a desktop session. Run the examples below from the repository root, replacing `input.png` and `sheet.png` with your own files.

Pixelate an image with a 64 × 64 size limit and up to 16 RGB colors:

```sh
cargo run --locked --release -p pixauro_cli -- pixelate input.png -o output.png --size 64x64 --colors 16 --background none
```

Split a sheet into two animations of four frames each, keeping the cropped pixels, and export a sprite sheet plus GIF previews:

```sh
cargo run --locked --release -p pixauro_cli -- animate sheet.png -o output --rows 2 --columns 4 --size 64x64 --fps 10 --no-pixelate --background none --gif
```

Choose a canvas large enough for each crop when using `--no-pixelate`.

For named animations, per-animation frame rates, or exact crop regions, use a JSON recipe:

```sh
cargo run --locked --release -p pixauro_cli -- animate sheet.png -o output --config cli/examples/animation.json
cargo run --locked --release -p pixauro_cli -- --help
```

See the examples for [pixelation](cli/examples/pixelate.json), [grid animations](cli/examples/animation.json), and [explicit slices and offsets](cli/examples/slices.json). Command-line options override the recipe; `--save-config recipe.json` saves the resolved settings to a new file. Progress and warnings go to stderr, and output paths go to stdout.

## File formats

| Format                  | Use                                                                                                               |
| ----------------------- | ----------------------------------------------------------------------------------------------------------------- |
| Project JSON            | Editable artwork, palette, layers, animations, and edit history.                                                  |
| PNG                     | Image input; frame, palette, and sprite sheet output.                                                             |
| Sprite sheet PNG + JSON | Exported frames with rectangles, animation names, frame order, and FPS. Uses the `pixauro-spritesheet` v1 format. |
| GIF                     | Looping animation previews exported by the CLI with `--gif`.                                                      |

Project JSON and sprite sheet JSON serve different purposes. Keep the project file if you want to continue editing. Sprite sheet metadata uses Pixauro's own schema; importing it into another tool may require an adapter.

## Development

Pixauro uses Rust for document state and image processing, Tauri 2 for the desktop host, and React with TypeScript for the interface.

| Directory               | Role                                                               |
| ----------------------- | ------------------------------------------------------------------ |
| `engine/`               | Documents, edit history, image I/O, and the shared asset pipeline. |
| `editor/`               | React interface and the Tauri host in `src-tauri/`.                |
| `cli/`                  | Command-line pixelation and animation export.                      |
| `pixelart-abstraction/` | Pixelation algorithm with transparency support.                    |

Read [AGENTS.md](AGENTS.md) before making changes, and [DESIGN.md](DESIGN.md) for interface work. Both are currently written in Chinese. From the repository root, run:

```sh
sh tools/check.sh
```

This checks Rust formatting and Clippy, generated IPC bindings, ESLint, Prettier, TypeScript, and the frontend build. It does not run automated tests; check behavior manually in the desktop app or CLI.

Bug reports, small fixes, and documentation improvements are welcome. For a bug report, include your OS, the commit you built, steps to reproduce, and a small input file when possible. For larger changes, open an issue to discuss the scope first.

## Algorithm notes

The pixelation module independently implements the approach described by Gerstner et al. in _Pixelated Image Abstraction with Integrated User Constraints_, with local extensions for transparency. See the [algorithm notes](pixelart-abstraction/docs/algorithm.md) for implementation choices and differences from the paper. The local extensions do not carry the same validation as the published method.

## License

Pixauro is licensed under the [MIT License](LICENSE). The `pixelart-abstraction` module retains its [own MIT copyright notice](pixelart-abstraction/LICENSE).
