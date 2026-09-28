# Template

Copy this folder to `plugins/NAME`, rename the crate in `Cargo.toml` to `kalem-plugin-NAME`, fill `plugin.json` (the fields are those of section 11.5 of Kalem's design document), and put your code in `src/lib.rs`.

The plugin API bindings (`kalem-plugin`, generated from the WIT definition) are not published yet. Until they are, this template holds the manifest, the conformance test and the build so that CI is green; the stub in `src/lib.rs` says where the code goes.

```sh
cargo test -p kalem-plugin-template
cargo build --release --target wasm32-wasip2 -p kalem-plugin-template
```
