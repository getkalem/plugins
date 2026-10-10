# Template

The crate a new plugin starts from: a viewer of a small format, each part of the contract in its place in `src/lib.rs`. A viewer's format is one that is not text, since Kalem opens a text file in a mode, never in a viewer; this one is a picture, the bytes `DOTS` and a NUL (the first bytes `plugin.json` declares in `applies`), then rows of `#` and `.`. `kalem plugin new NAME`, run anywhere in this repository, copies this folder to `plugins/NAME` and names everything for NAME; then fill `plugin.json` (the fields are those of section 11.5 of Kalem's design document) and replace the format with yours. The Book's "Plugins in practice" walks through it.

```sh
cargo test -p kalem-plugin-template
kalem plugin build template
kalem plugin install template
printf 'DOTS\0.##.\n#..#\n.##.\n' > a.template
kalem view a.template
```

`kalem plugin new NAME`, run anywhere in this repository, copies this folder to `plugins/NAME` and renames it. `kalem plugin build DIR` compiles the crate for `wasm32-unknown-unknown` (`rustup target add wasm32-unknown-unknown` once) and wraps the module as the component `main` names in `plugin.json`; CI does the same with `wasm-tools component new`. Built for `wasm32-wasip2` a component would import WASI interfaces, which Kalem does not grant.
