//! The integration tests of the core: one binary, a module per area, through the public API only.
//! A test stays next to its code only when no public route reaches what it checks (a private kernel, a `pub(crate)`
//! item); an item is never made `pub` to move a test out. Run one area: `cargo test -p bullterrier-audio --test it edits::`.

mod common;

mod architecture;
mod edits;
mod formats;
mod measures;
mod sound;
