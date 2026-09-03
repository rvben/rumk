//! Makefile bytes as Make reads them.

/// The byte order mark GNU Make 4.3 and later skip before reading the first
/// line. Older Make reads it as part of the first word on that line, which
/// turns `.PHONY` into a target name of its own.
pub const BYTE_ORDER_MARK: &str = "\u{feff}";

/// The bytes of a Makefile without a leading byte order mark, and whether one
/// stood there. Rules read the text Make reads, and a rewrite puts the mark
/// back so the file keeps the bytes it came with.
pub fn split_byte_order_mark(bytes: &[u8]) -> (&[u8], bool) {
    match bytes.strip_prefix(BYTE_ORDER_MARK.as_bytes()) {
        Some(text) => (text, true),
        None => (bytes, false),
    }
}
