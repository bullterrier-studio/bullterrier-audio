`bundle-1.schema.json` and `bundle-2.schema.json` are copies of bullterrier-control's `schemas/bundle-N.schema.json`
(bundle-1: commit 9d74648, 2026-10-29; bundle-2: commit ebbd87b, 2026-11-12), unedited. Version 2 adds `grid.confidence`.
A producer validates a document against the schema named by its `version`: 1 when the grid has no confidence, 2 when it has.
A change to the format starts in control; then the file is copied again here. The writer in `crates/core/src/formats.rs` and
`crates/core/tests/it/formats.rs` (which validates every output against the schema of its version) follow it.
