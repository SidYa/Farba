# Third-party notices

Farba is licensed under the GNU General Public License, version 3 or later, with an
additional permission for Microsoft DirectML (see `LICENSE`). It contains or uses the
third-party works listed below, under their own licences.

## Pinta and Pinta-plus (MIT)

Parts of Farba are ported from Pinta and from Pinta-plus (a fork of Pinta 3.3):

| Farba component | Source | Used for |
|---|---|---|
| Colour picker dialog (`app/ui/src/components/color/`) | Pinta `Pinta.Gui.Widgets/Widgets/ColorPickerDialog.cs`, `ColorPickerSlider.cs` (via Pinta-plus) | Layout, wheel / square maths and behaviour |
| Default palette (`DEFAULT_PALETTE` in `app/ui/src/lib/palette.svelte.ts`) | Pinta `Pinta.Core/Extensions/PaletteHelper.cs` | The 34 palette colours |
| Colors pad (`app/ui/src/components/panels/ColorsPad.svelte`) | Pinta-plus `ColorPalettePanelWidget.cs` | Geometry and behaviour |
| AI background removal (`core/farba-ai`) | Pinta-plus `Pinta.AiBackgroundRemoval` (BackgroundRemover, ModelStore, BackgroundRemovalModel) | Model catalogue, download checks, session handling and pipeline |

```
The MIT License

Copyright (c) 2010 Jonathan Pobst <monkey@jpobst.com>

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in
all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
THE SOFTWARE.
```

Parts of Pinta are derived from Paint.NET 3.36, which is licensed under the same MIT
licence: Copyright (C) 2007 Rick Brewster, Chris Crosetto, Tom Jackson, Michael Kelsey,
Brandon Ortiz, Craig Taylor, Chris Trevino, and Luke Walker. Portions Copyright (C) 2007
Microsoft Corporation.

## GIMP (GPL-3.0-or-later)

GIMP's behaviour is the reference for most tools. No GIMP code is ported at the time of
writing; ported parts will be listed here.

## Libraries with notable licences

| Library | Licence | Used for |
|---|---|---|
| `ravif`, `avif-decode` | BSD-3-Clause | AVIF encode / decode |
| `rav1e`, `rav1d` | BSD-2-Clause | AV1 encoder / decoder |
| `resvg`, `usvg` | MIT / Apache-2.0 | SVG import |
| `tiny-skia` | BSD-3-Clause | SVG rasterisation |
| `pdf-writer` | MIT / Apache-2.0 | PDF export |
| `cosmic-text`, `fontdb`, `swash` | MIT / Apache-2.0 | Text layout and rendering |
| `ort` + ONNX Runtime | MIT / Apache-2.0, MIT | AI add-on inference |
| DirectML (`DirectML.dll`, Microsoft) | Microsoft DirectML licence | AI add-on, GPU inference on Windows |
| Material Symbols (Google) | Apache-2.0 | Icons |
| Tauri, Svelte | MIT / Apache-2.0, MIT | Application shell, user interface |

Linux build only: `pdfium-render` (MIT / Apache-2.0) and PDFium (BSD-3-Clause /
Apache-2.0), built by `pdfium-binaries` (MIT); their licence texts ship in the package.

The HEIC add-on carries libheif and libde265 (LGPL-3.0, unmodified DLLs) with their
licence texts and `THIRD-PARTY.txt` (versions and source links).

The other Rust crates and npm packages are under MIT, Apache-2.0, BSD or Zlib licences.

BiRefNet models (MIT) are downloaded at runtime by the AI add-on and are not
redistributed.
