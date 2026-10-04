# PDF viewer

PDF files (ISO 32000-2) opened as themselves (D55): pages shown as the file draws them, never converted. A plugin of Kalem's `document-viewer` contract (D54, Kalem's task T3.7.3); Kalem bundles it (D28) until components load (T3.1.12).

## Renderer

[hayro](https://github.com/LaurenzV/hayro), a pure-Rust rasterizer (D56), with its default features: the base-14 fonts' substitutes (Foxit's CFF fonts) and the predefined CMaps are embedded, so a file that does not embed its fonts draws inside the sandbox too. A page is rendered at the scale the host asks for (1 is 72 dpi), so it stays sharp when zoomed; the last six renders are kept. A page is rendered at most 8,192 pixels on its longer side and 40 megapixels in all, its scale lowered to fit.

The page shown is the crop box, turned by `Rotate`, as Acrobat shows it. Annotations and form fields are drawn from their appearance streams; hidden annotations are not.

A dark theme maps the page's lightness between the theme's background and foreground and keeps its colors' differences from gray, so white paper takes the background, black ink the foreground, and a figure keeps its hues.

## What it reads

| Part | Specification | Notes |
|---|---|---|
| Pages | 7.7.3 | Each page a unit |
| Page labels | 12.4.2 | Decimal, Roman, letters, prefixes and starts from the `PageLabels` number tree; a page without a label is its number |
| Outline | 12.3.3 | Titles in PDFDocEncoding, UTF-16 or UTF-8; explicit destinations, named destinations (the catalog's `Dests` and the `Dests` name tree) and `GoTo` actions; an entry without a destination points where the one before it does |
| Links | 12.5.6.5 | `URI` actions as URLs; destinations and `GoTo` actions as `#N` (N the page's index, from 0); `GoToR` and `Launch` as their file names; rectangles in the page's pixels at scale 1 |
| Document information | 14.3.3 | Title, author, subject, keywords, creator, producer and dates in the information panel, with the version, the page count, the first page's size (with its paper's name), the file size, and whether the file is encrypted |
| Encryption | 7.6 | RC4 and AES through hayro; a file with a user password fails with `PASSWORD_REQUIRED`, and `PdfViewer::open_with_password` opens it |

Every walk of the catalog (outline, name and number trees) is bounded and remembers the objects it visited, so a file with a cycle opens.

## Text

Each glyph's Unicode comes from hayro (`ToUnicode`, then the glyph's name). Glyphs outside the crop box are left out, as is a space glyph the next glyph is drawn over. The glyphs drawn in a row make runs; runs on one baseline that touch make a line, a superscript included; lines are read top to bottom, a column at a time where consecutive rows share a gutter at least an em wide and each column is at least eight ems wide (narrower columns are a table, read a row at a time). A glyph drawn twice over itself (fake bold) counts once; a space goes where two glyphs stand more than 0.15 em apart; the Latin ligatures are spelled out (`ﬁ` is `fi`) so that search finds them.

Search reads every page's text once, fonts parsed once for all pages, and keeps it.

## What it edits

Nothing yet. Highlights, notes and form filling written as incremental updates appended after the original bytes are Kalem's task T3.7.3b.

## As a component

The crate is a viewer of the Rust contract, which Kalem bundles, and on `wasm32` a component of the WIT world `document-viewer` through `kalem_plugin::export_viewer_of!`: it reads the file through the handle Kalem gives it and nothing else.

```sh
kalem plugin build plugins/pdf-viewer
kalem plugin install plugins/pdf-viewer
```

Installed, the component takes the place of the bundled viewer.

## Tests

`cargo test -p kalem-plugin-pdf-viewer`: PDF files the tests write themselves (no file whose license would need recording): detection, page labels, the outline through each kind of destination, text, search and links, the information panel, rendering at scales, a turned page and the dark theme, a page past the memory budget, an outline with a cycle, and every cut of a file failing without a panic; the manifest's conformance.

`cargo run --release --example pdf -- FILE [--text] [--page N] [--scale S] [--ppm OUT]` prints what the viewer reads of a file, with timings, and writes a page as PPM for comparison with `pdftoppm`.

Measured against poppler on 84 pages of 21 books (O'Reilly, No Starch, Pragmatic, Manning, Addison-Wesley, a magazine; 2026-10-03): the words of the text agree with `pdftotext -cropbox` at a similarity of 0.993 on average and never below 0.96; pages render at `pdftoppm -cropbox -r 72`'s size, with a mean difference of 1 to 9 levels a channel (anti-aliasing). A 1,245-page book opens in 9 ms and shows its first page in 18 ms; searching a 766-page book takes about a second the first time.

Not yet: a corpus of pdf.js's test files compared with pdfium as the oracle (T3.7.3a), pixel similarity per page in CI, and the conformance suite against the fake host (T3.1.17).
