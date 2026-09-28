//! A Kalem plugin.
//!
//! Kalem loads a plugin as a WebAssembly component and calls it through the
//! contracts of its design document: a mode (11.11), a completer (11.12),
//! commands and the other extension points (11.10). The bindings for those
//! contracts are generated from Kalem's WIT definition and published as the
//! `kalem-plugin` crate (Kalem's task T3.1.3). When they are available, the
//! plugin implements the traits here and exports them with the bindings'
//! macro; until then this crate carries only its manifest and its tests.

/// The manifest, embedded so that the component carries its own description.
pub const MANIFEST: &str = include_str!("../plugin.json");

#[cfg(test)]
mod tests {
    use super::MANIFEST;

    #[test]
    fn manifest_is_json() {
        let value: serde_json::Value = serde_json::from_str(MANIFEST).expect("plugin.json parses");
        assert!(value.is_object());
    }
}
