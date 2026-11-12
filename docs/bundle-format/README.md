`bundle-1.schema.json` is a copy of bullterrier-control's `schemas/bundle-1.schema.json` (commit 9d74648, 2026-10-29), unedited.
A change to the format starts in control; then the file is copied again here. The writer in `crates/core/src/formats.rs` and
`crates/core/tests/it/formats.rs` (which validates every output against this file) follow it.
