"""The Rust core's home in the package: `_core` here is the extension built from crates/python (`_core.pyi` is its
stub). Internal, and below every layer but `errors`: the extension imports `bullterrier_audio.errors` (its
`InvalidError`) when it first raises one, and nothing else of the package, so every layer above may use it. Callers
write `from bullterrier_audio._native import _core`; the import linter sees this package where it cannot see a
compiled module.
"""
