//! Architecture tests: they read the crate's own source as code (which a behaviour test never does).
//! The core is pure Rust: it never names `PyO3` or numpy, so it builds anywhere, a browser included. The scan has rows
//! that prove it sees what it must and only that.

use std::fs;
use std::path::Path;

use crate::common::rust_files;

/// The lines of `text` that name `PyO3` or numpy in code (a `//` comment, a doc comment included, names them freely).
fn python_in_code(text: &str) -> Vec<&str> {
    text.lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .filter(|line| line.contains("pyo3") || line.contains("numpy"))
        .collect()
}

#[test]
fn the_core_never_names_python() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let files = rust_files(&source);
    assert!(!files.is_empty(), "no source under {}", source.display());
    for file in files {
        let text = fs::read_to_string(&file).unwrap_or_else(|error| unreachable!("{}: {error}", file.display()));
        assert_eq!(python_in_code(&text), Vec::<&str>::new(), "{}", file.display());
    }
}

#[test]
fn the_scan_sees_code_that_names_python_and_not_a_comment_that_does() {
    assert_eq!(python_in_code("use pyo3::prelude::*;\n"), ["use pyo3::prelude::*;"]);
    assert_eq!(
        python_in_code("let array = numpy::zeros();\n"),
        ["let array = numpy::zeros();"]
    );
    assert_eq!(
        python_in_code("//! No PyO3 and no numpy here.\n    // pyo3\nfn f() {}\n"),
        Vec::<&str>::new()
    );
}
