# Contributing a plugin

- Read the design document of Kalem, sections 11.0 to 11.12: what a plugin can add, the contracts it implements, the sandbox and its budgets.
- One plugin, one crate, one directory under `plugins/`. The crate is named `kalem-plugin-NAME`; the directory is `NAME`; the tag that publishes it is `NAME-vX.Y.Z`.
- `cargo fmt --all` and `cargo clippy --workspace --all-targets` must be clean; CI treats warnings as errors.
- Never commit a compiled component. The release workflow builds it from the tagged source.
- A plugin declares in `plugin.json` every permission it needs, and nothing it does not: a mode that asks for the network is a red flag for reviewers.
- A mode returns ranges into the text, never text: the round trip is Kalem's guarantee, and the conformance suite checks it.
- Keep the README of your plugin honest about what works in the graphical editor, in the terminal editor and in batch mode. The terminal is never second class.
- Add yourself to `CODEOWNERS` for your plugin's directory.

By contributing you agree that your contribution is licensed under MIT OR Apache-2.0.
