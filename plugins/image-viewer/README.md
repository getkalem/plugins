# Image viewer

Pictures opened as themselves (D55): a picture is shown, never converted, and the only edits are those the format allows without encoding the picture again. The first plugin of Kalem's `document-viewer` contract (D54, Kalem's task T3.7.2); Kalem bundles it (D28) until components load (T3.1.12).

## What it opens

| Format | Decoder | Notes |
|---|---|---|
| PNG, APNG | `image` (png) | APNG frames are played |
| JPEG | `image` (zune-jpeg) | EXIF orientation honored; the only format edited |
| GIF | `image` (gif) | Animation frames with their delays (under 20 ms counts as 100 ms, as browsers do) |
| WebP | `image` (image-webp) | Lossy, lossless and animated |
| BMP, ICO, QOI, TGA, PNM (PBM, PGM, PPM, PAM), TIFF | `image` | TIFF orientation honored |
| DDS | `image` | DXT1, DXT3, DXT5 |
| OpenEXR, Radiance HDR | `image` (exr, hdr) | Linear light tone-mapped to sRGB (Reinhard) |
| SVG | `resvg` | Drawn at its size, a small drawing at least 1,024 pixels on its longer side; text needs fonts, which a sandboxed plugin does not have |

A file is detected by its first bytes, then by its extension (TGA has no magic number). In Kalem, an SVG file is text and opens in a text mode; the viewer shows it while stepping through a folder's pictures.

## Memory

A picture is kept at most 8,192 pixels on its longer side (a GPU texture's limit) and 40 megapixels over all its frames; a larger one is scaled down right after decoding, so a 100-megapixel photograph opens. The information panel then gives both sizes.

## What it edits

A JPEG is turned right or left and flipped horizontally or vertically by rewriting its EXIF orientation tag (CIPA DC-008), never by encoding it again:

- the tag present: its two bytes are the only bytes that change;
- IFD0 without the tag: a copy of IFD0 with the tag is appended to the TIFF data and the header points to it, so no offset moves;
- no EXIF: an Exif segment holding the tag alone is inserted after the JFIF segment.

Each edit has its inverse for Kalem's undo. Nothing else is offered for any format.

## Information panel

Format, size, file size, color type as stored, ICC profile, frames and duration, orientation, and from EXIF (read by `kamadak-exif`): camera, lens, date taken, exposure, aperture, ISO, focal length, color space, position, software, artist, copyright.

## As a component

The crate is a viewer of the Rust contract, which Kalem bundles, and on `wasm32` a component of the WIT world `document-viewer` through `kalem_plugin::export_viewer_of!`: it reads the file through the handle Kalem gives it and nothing else.

```sh
kalem plugin build plugins/image-viewer
kalem plugin install plugins/image-viewer
```

Installed, the component takes the place of the bundled viewer.

## Tests

`cargo test -p kalem-plugin-image-viewer`: one file per format made by the test itself (no picture whose license would need recording) and decoded against the pixels it was made from; the eight orientations; a turn and a save that differ from the original in the tag only; an animated GIF; a 50-megapixel picture under the budget; the manifest's conformance.

Not yet: hashes against reference decoders (libpng, libjpeg-turbo, giflib) on a corpus of real files, and the conformance suite against the fake host (T3.1.17).
