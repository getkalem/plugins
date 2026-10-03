# Template

Copy this folder to `plugins/NAME`, rename the crate in `Cargo.toml` to `kalem-plugin-NAME`, fill `plugin.json` (the fields are those of section 11.5 of Kalem's design document), and put your code in `src/lib.rs`.

The plugin API bindings (`kalem-plugin`, generated from the WIT definition) are not published yet. Until they are, this template holds the manifest, the conformance test and the build so that CI is green; the stub in `src/lib.rs` says where the code goes.

```sh
cargo test -p kalem-plugin-template
kalem plugin build template
```

`kalem plugin new NAME`, run anywhere in this repository, copies this folder to `plugins/NAME` and renames it. `kalem plugin build DIR` compiles the crate for `wasm32-unknown-unknown` (`rustup target add wasm32-unknown-unknown` once) and wraps the module as the component `main` names in `plugin.json`; CI does the same with `wasm-tools component new`. Built for `wasm32-wasip2` a component would import WASI interfaces, which Kalem does not grant.
