//! Declares the `kalem_layer` cfg: the component's layer over Kalem's
//! Markdown and Org (plugin API 0.2.10) is built with
//! `RUSTFLAGS="--cfg kalem_layer"` against a Kalem whose `kalem-plugin`
//! has the feature `layer`, until Kalem's `main` has it (graph_todo GR4b).

fn main() {
    println!("cargo::rustc-check-cfg=cfg(kalem_layer)");
}
