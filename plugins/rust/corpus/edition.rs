//! Rust as the 2021 and 2024 editions write it, for the plugin's
//! highlighting tests: the constructs Rust gained after the syntax built
//! into Kalem's highlighter was written, a few to a line. It compiles as
//! a library (`rustc --edition 2024 --crate-type lib`) and does nothing.

#![allow(dead_code, unused, async_fn_in_trait, deprecated)]

use std::ffi::{CStr, c_char};
use std::fmt::Display;

/// A trait with an `async fn` and a method returning `impl Trait`.
pub trait Shape {
    /// Its size, loaded.
    async fn load(&self) -> u32;
    /// Its names.
    fn names(&self) -> impl Iterator<Item = &str>;
    /// A generic associated type.
    type Part<'a>
    where
        Self: 'a;
}

/// An attribute of the diagnostic namespace.
#[diagnostic::on_unimplemented(message = "`{Self}` has no area")]
pub trait Area: Send + Sync {
    /// The area.
    fn area(&self) -> f64;
}

/// `let`–`else` and a let chain.
pub fn second_if_even(v: &[i32]) -> Option<i32> {
    let [first, ..] = v else {
        return None;
    };
    if let Some(x) = v.get(1)
        && *x % 2 == 0
        && *first > 0
    {
        return Some(*x);
    }
    None
}

/// A labeled block.
pub fn labeled(n: u32) -> u32 {
    'found: {
        if n > 10 {
            break 'found 1;
        }
        0
    }
}

/// C strings, raw strings, byte strings and characters.
pub const GREETING: &CStr = c"hello";
pub const RAW_C: &CStr = cr#"a "quoted" word"#;
pub const RAW: &str = r#"a "raw" string"#;
pub const BYTES: &[u8] = br"bytes\n";
pub const BYTE: u8 = b'x';
pub const CRAB: char = '\u{1F980}';
pub const NUMBERS: [f64; 4] = [1_000.0, 0x_ff as f64, 1e-3, 0b1010 as f64];

/// An inline `const` block and a const generic.
pub fn zeros<const N: usize>() -> [u8; N] {
    const { assert!(N < 1024) };
    [0; N]
}

/// Precise capturing.
pub fn chars<'a>(s: &'a str) -> impl Iterator<Item = char> + use<'a> {
    s.chars()
}

/// An `async` closure and an `async` block.
pub async fn run() -> u32 {
    let add = async |x: u32| x + 1;
    let block = async move { 2 };
    add(1).await + block.await
}

/// Raw identifiers; `gen` is reserved since 2024.
pub fn r#gen(r#type: u32) -> u32 {
    r#type
}

unsafe extern "C" {
    /// A foreign function safe to call.
    pub safe fn abs(x: i32) -> i32;
    /// One that is not.
    pub unsafe fn strlen(p: *const c_char) -> usize;
}

/// An unsafe attribute.
#[unsafe(no_mangle)]
pub extern "C" fn kalem_corpus_answer() -> i32 {
    42
}

/// A raw borrow.
pub fn raw_pointer() -> *const u32 {
    static VALUE: u32 = 7;
    &raw const VALUE
}

/// Lifetimes, a `where` clause, `?Sized`.
pub fn longest<'a, T>(a: &'a T, b: &'a T) -> &'a T
where
    T: PartialOrd + ?Sized,
{
    if a > b { a } else { b }
}

/// `dyn` with a lifetime, a turbofish, a closure.
pub fn shown(items: &[Box<dyn Display + '_>]) -> String {
    items
        .iter()
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Exclusive range patterns.
pub fn class(n: u8) -> &'static str {
    match n {
        0 => "zero",
        1..10 => "digit",
        10..100 => "two digits",
        _ => "more",
    }
}

/// A lint expected, and a format string's arguments.
#[expect(unused_variables)]
pub fn formatted(width: usize) -> String {
    let unused = 1;
    let name = "n";
    format!("{name:>width$} {0:?} {1:#x}", 1.5, 255)
}

/// Every fragment specifier.
///
/// ```
/// assert_eq!(every!(1, 2, 3), 3);
/// ```
#[macro_export]
macro_rules! every {
    ($i:ident, $e:expr, $f:expr_2021, $t:ty, $p:pat, $q:pat_param, $s:stmt) => {};
    ($b:block, $l:lifetime, $li:literal, $pa:path, $m:meta, $v:vis, $it:item) => {};
    ($($x:expr),* $(,)?) => { [$($x),*].len() };
}

/// Nested attribute arguments.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(test, derive(PartialOrd))]
#[deprecated(since = "0.1.0", note = "use `Shape`")]
#[repr(u8)]
pub enum Kind {
    #[default]
    Plain = 1,
    Bold,
}

/// A union.
#[repr(C)]
pub union Bits {
    int: u32,
    float: f32,
}
