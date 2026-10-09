//! A borrow error, on purpose: the plugin's diagnostics are checked on it.
//! This file does not compile.

#[test]
fn pushes_while_borrowed() {
    let mut sizes = vec![1.0, 2.0];
    let first = &sizes[0];
    sizes.push(3.0);
    assert_eq!(*first, 1.0);
}
