#!/usr/bin/env -S cargo +nightly -Zscript
---
[package]
edition = "2024"

[dependencies]
---

//! A single-file package (Cargo's `-Zscript`, nightly's), for the
//! highlighting tests: the `#!` line and the TOML frontmatter.

fn main() {
    println!("{}", env!("CARGO_PKG_NAME"));
}
