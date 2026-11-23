//! A Rust port of the predicator condition language, which compiles a
//! condition to a small instruction set and evaluates it on a stack machine.
//!
//! This release is a placeholder. The crate exports nothing yet.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

#[cfg(test)]
mod tests {
    use unicode_segmentation::UnicodeSegmentation;

    // The empty crate has no code of its own, and the coverage stage refuses a
    // build with no instrumented line, so this test is the only line it
    // measures until the library has code. It also proves the one required
    // dependency resolves.
    // Sabotage: with the expected count changed from 2 to 3, this test fails.
    #[test]
    fn links_the_segmentation_dependency() {
        assert_eq!("e\u{301}x".graphemes(true).count(), 2);
    }
}
