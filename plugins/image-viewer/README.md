# Image viewer

Pictures opened as themselves (D55): a picture is shown, never converted, and the only edits are those the format allows without encoding the picture again. The first plugin of Kalem's `document-viewer` contract (D54, Kalem's task T3.7.2). Kalem has no viewer built in: install this one from Kalem's plugin list, from the offer Kalem makes the first time you open such a file, or with `kalem plugin install image-viewer`.

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
| SVG | `resvg` | Drawn at its size, a small drawing at least 1,024 pixels on its longer side; its text and the pictures inside it are not drawn (`resvg` is built without them) |

Kalem opens a file with this viewer by its extension (`opens` in `plugin.json`). The plugin then picks the decoder by the file's first bytes, else by its extension (TGA has no magic number), so a PNG named `.jpg` still opens. The orientation a decoder reports is honored in every format: JPEG, TIFF, PNG and WebP. In Kalem, an SVG file is text and opens in a text mode; the viewer shows it while stepping through a folder's pictures.

## Memory

A picture is kept at most 8,192 pixels on its longer side (a GPU texture's limit) and 40 megapixels over all its frames; a larger one is scaled down right after decoding, so a 100-megapixel photograph opens. An animation's frames are counted first and each is scaled as it is decoded, so a long one never stands whole at full size. The information panel then gives both sizes.

## What it edits

A JPEG is turned right or left and flipped horizontally or vertically by rewriting its EXIF orientation tag (CIPA DC-008), never by encoding it again:

- the tag present: only the tag's value changes;
- IFD0 without the tag: a copy of IFD0 with the tag is appended to the TIFF data and the header points to it, so no offset inside the TIFF data moves;
- no EXIF: an Exif segment holding the tag alone is inserted after the JFIF segment (or right after the start of the file when there is none).

Each edit has its inverse for Kalem's undo. Nothing else is offered for any format. In Kalem, the command Edit the File applies these edits (Rotate Right, Rotate Left, Flip Horizontally, Flip Vertically), and Save writes them; Rotate View Right and Left (`r`, `Shift+R`) only turn the view, in any format, and change nothing in the file.

## Information panel

Format, size, file size, color type as stored, ICC profile, frames and duration, orientation, and from EXIF (read by `kamadak-exif`): camera, lens, date taken, exposure, aperture, ISO, focal length, color space, position, software, artist, copyright.

## As a component

The crate is a viewer of Kalem's Rust contract (`kalem-viewer`'s `Viewer`), and on `wasm32` a component of the WIT world `document-viewer` through `kalem_plugin::export_viewer_of!`: it reads the file through the handle Kalem gives it and nothing else.

```sh
kalem plugin build plugins/image-viewer
kalem plugin install plugins/image-viewer
```

These commands build and install it from this folder; the released plugin is installed with `kalem plugin install image-viewer`.

## Tests

`cargo test -p kalem-plugin-image-viewer`: one file per format made by the test itself (no picture whose license would need recording) and decoded against the pixels it was made from; the eight orientations; a turn and a save that differ from the original in the tag only; an animated GIF; a 50-megapixel picture and a 48-megapixel animation under the budget; the manifest's conformance.

Not yet: hashes against reference decoders (libpng, libjpeg-turbo, giflib) on a corpus of real files, and the conformance suite against the fake host (T3.1.17).
