//! The ONNX Runtime library: which release the package runs models with, where it is kept, and getting it.
//!
//! None is bundled. A release is pinned per platform with its size and SHA-256 ([`runtimes`]); [`install_runtime`]
//! downloads the archive (see [`fetch`]), takes the one library file out of it and keeps that file in the cache folder,
//! where [`runtime_library`] finds it. The library file is loaded, never run.

use std::path::{Path, PathBuf};

use super::download::fetch;
use super::{Options, Status};
use crate::Error;

/// The release of ONNX Runtime the package pins.
pub const RUNTIME_VERSION: &str = "1.23.2";

/// How a release is packed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Archive {
    /// A gzip-compressed tar (macOS and Linux).
    TarGz,
    /// A zip (Windows).
    Zip,
}

/// A release of ONNX Runtime for one platform.
#[derive(Debug, Clone, Copy)]
pub struct Runtime {
    /// The operating system, as `std::env::consts::OS` names it: `macos`, `linux`, `windows`.
    pub os: &'static str,
    /// The processor, as `std::env::consts::ARCH` names it: `aarch64`, `x86_64`.
    pub arch: &'static str,
    /// Where the archive is downloaded from.
    pub url: &'static str,
    /// The SHA-256 of the archive, as 64 lowercase hex digits.
    pub sha256: &'static str,
    /// The size of the archive in bytes.
    pub size: u64,
    /// How the archive is packed.
    pub archive: Archive,
    /// The name of the library file inside it (in its `lib` folder), which is also the name it is kept under.
    pub file: &'static str,
}

/// The releases the package pins, one per platform ONNX Runtime publishes for.
#[must_use]
pub fn runtimes() -> &'static [Runtime] {
    &[
        Runtime {
            os: "macos",
            arch: "aarch64",
            url: "https://github.com/microsoft/onnxruntime/releases/download/v1.23.2/onnxruntime-osx-arm64-1.23.2.tgz",
            sha256: "b4d513ab2b26f088c66891dbbc1408166708773d7cc4163de7bdca0e9bbb7856",
            size: 9_999_931,
            archive: Archive::TarGz,
            file: "libonnxruntime.1.23.2.dylib",
        },
        Runtime {
            os: "macos",
            arch: "x86_64",
            url: "https://github.com/microsoft/onnxruntime/releases/download/v1.23.2/onnxruntime-osx-x86_64-1.23.2.tgz",
            sha256: "d10359e16347b57d9959f7e80a225a5b4a66ed7d7e007274a15cae86836485a6",
            size: 11_676_322,
            archive: Archive::TarGz,
            file: "libonnxruntime.1.23.2.dylib",
        },
        Runtime {
            os: "linux",
            arch: "x86_64",
            url: "https://github.com/microsoft/onnxruntime/releases/download/v1.23.2/onnxruntime-linux-x64-1.23.2.tgz",
            sha256: "1fa4dcaef22f6f7d5cd81b28c2800414350c10116f5fdd46a2160082551c5f9b",
            size: 8_309_231,
            archive: Archive::TarGz,
            file: "libonnxruntime.so.1.23.2",
        },
        Runtime {
            os: "linux",
            arch: "aarch64",
            url: "https://github.com/microsoft/onnxruntime/releases/download/v1.23.2/onnxruntime-linux-aarch64-1.23.2.tgz",
            sha256: "7c63c73560ed76b1fac6cff8204ffe34fe180e70d6582b5332ec094810241e5c",
            size: 7_254_068,
            archive: Archive::TarGz,
            file: "libonnxruntime.so.1.23.2",
        },
        Runtime {
            os: "windows",
            arch: "x86_64",
            url: "https://github.com/microsoft/onnxruntime/releases/download/v1.23.2/onnxruntime-win-x64-1.23.2.zip",
            sha256: "0b38df9af21834e41e73d602d90db5cb06dbd1ca618948b8f1d66d607ac9f3cd",
            size: 78_127_794,
            archive: Archive::Zip,
            file: "onnxruntime.dll",
        },
        Runtime {
            os: "windows",
            arch: "aarch64",
            url: "https://github.com/microsoft/onnxruntime/releases/download/v1.23.2/onnxruntime-win-arm64-1.23.2.zip",
            sha256: "1cfe88b6435df3b5fb0e9f6bd7d6f5df1e887b6174de7f6e2a47bab956f3f168",
            size: 78_932_411,
            archive: Archive::Zip,
            file: "onnxruntime.dll",
        },
    ]
}

/// The release for this platform, or the error saying what to do instead.
fn here() -> Result<&'static Runtime, Error> {
    let (os, arch) = (std::env::consts::OS, std::env::consts::ARCH);
    runtimes().iter().find(|runtime| runtime.os == os && runtime.arch == arch).ok_or_else(|| {
        Error::ModelUnavailable(format!(
            "no ONNX Runtime {RUNTIME_VERSION} is pinned for {os} on {arch}: set BT_AUDIO_ONNX_LIB to the file of a library \
             you have (libonnxruntime.so, libonnxruntime.dylib or onnxruntime.dll)"
        ))
    })
}

/// Where `runtime`'s library is kept under the cache folder of `options`.
fn kept(runtime: &Runtime, options: &Options) -> Result<PathBuf, Error> {
    Ok(options
        .cache()?
        .join(format!("onnxruntime-{RUNTIME_VERSION}"))
        .join(runtime.file))
}

/// Whether the ONNX Runtime library is available: the file `options` name, else the one kept in the cache folder. The
/// library file has no pinned checksum (the archive it came in did), so a file is installed if it is a file.
///
/// # Errors
/// [`Error::ModelUnavailable`] when the cache folder is not known, or no release is pinned for this platform and none is named.
pub fn runtime_status(options: &Options) -> Result<Status, Error> {
    let (path, named) = match options.library() {
        Some(named) => (named.to_path_buf(), true),
        None => (kept(here()?, options)?, false),
    };
    Ok(if path.is_file() {
        Status::Installed(path)
    } else if named && path.exists() {
        Status::Damaged {
            path,
            reason: "it is not a file".to_owned(),
        }
    } else {
        Status::NotInstalled(path)
    })
}

/// The ONNX Runtime library file to load: the one `options` name, else the one kept in the cache folder; downloaded
/// first if it is not there and `options` allow.
///
/// # Errors
/// [`Error::ModelUnavailable`] saying to run `bt-audio install`, allow downloading (`BT_AUDIO_ALLOW_DOWNLOAD=1`) or set
/// `BT_AUDIO_ONNX_LIB` to a library file, when there is none and downloading is not allowed; otherwise as
/// [`install_runtime`].
pub fn runtime_library(options: &Options) -> Result<PathBuf, Error> {
    if let Some(named) = options.library() {
        return Ok(named.to_path_buf());
    }
    match runtime_status(options)? {
        Status::Installed(path) => Ok(path),
        Status::NotInstalled(path) | Status::Damaged { path, .. } if !options.downloads() => {
            Err(Error::ModelUnavailable(format!(
                "the ONNX Runtime library is not installed (expected at {}): run `bt-audio install`, or allow downloading it \
             (BT_AUDIO_ALLOW_DOWNLOAD=1), or set BT_AUDIO_ONNX_LIB to a library file",
                path.display()
            )))
        }
        _ => install_runtime(options),
    }
}

/// Downloads the ONNX Runtime release for this platform (verified), takes its library out and keeps it in the cache
/// folder of `options`; returns its path. A library already kept is not fetched again. This downloads whatever `options`
/// allow: the caller has decided.
///
/// # Errors
/// [`Error::ModelUnavailable`] when no release is pinned for this platform; [`Error::Write`] naming the file and the URL
/// when the download fails or does not verify, the archive holds no such library, or the file cannot be written.
pub fn install_runtime(options: &Options) -> Result<PathBuf, Error> {
    install_runtime_release(here()?, options)
}

/// [`install_runtime`] for a given release instead of the one of this platform.
///
/// # Errors
/// As [`install_runtime`].
pub fn install_runtime_release(runtime: &Runtime, options: &Options) -> Result<PathBuf, Error> {
    let target = kept(runtime, options)?;
    if target.is_file() {
        return Ok(target);
    }
    let fail = |why: String| Error::Write {
        path: target.clone(),
        reason: format!(
            "cannot install ONNX Runtime {RUNTIME_VERSION} from {}: {why}; download it by hand and save the library as {}",
            runtime.url,
            target.display()
        ),
    };
    let folder = target.parent().map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    std::fs::create_dir_all(&folder)
        .map_err(|error| fail(format!("cannot write to {} ({error})", folder.display())))?;
    let archive = folder.join(format!(".{}.download", runtime.file));
    fetch(runtime.url, runtime.sha256, runtime.size, &archive)?;
    let taken = take(runtime, &archive, &folder);
    drop(std::fs::remove_file(&archive)); // best effort: the archive is not needed once the library is out
    let temporary = taken.map_err(fail)?;
    std::fs::rename(&temporary, &target)
        .map_err(|error| fail(format!("cannot save {} ({error})", target.display())))?;
    Ok(target)
}

/// The library file of the archive at `archive`, written into `folder` under a temporary name; its path. No temporary
/// file stays when it fails.
fn take(runtime: &Runtime, archive: &Path, folder: &Path) -> Result<PathBuf, String> {
    let temporary = folder.join(format!(".{}.{}.part", runtime.file, std::process::id()));
    let copied = copy_library(runtime, archive, &temporary);
    if copied.is_err() {
        drop(std::fs::remove_file(&temporary));
    }
    copied.map(|()| temporary)
}

/// The library file of the archive at `archive` copied to `out`.
fn copy_library(runtime: &Runtime, archive: &Path, out: &Path) -> Result<(), String> {
    let damaged = |error: &dyn std::fmt::Display| format!("the archive is damaged ({error})");
    let unwritable = |error: std::io::Error| format!("cannot write ({error})");
    let opened = std::fs::File::open(archive).map_err(|error| format!("cannot read the archive ({error})"))?;
    let mut out = std::fs::File::create(out).map_err(unwritable)?;
    let mut copied = false;
    match runtime.archive {
        Archive::TarGz => {
            let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(opened));
            for entry in tar.entries().map_err(|error| damaged(&error))? {
                let mut entry = entry.map_err(|error| damaged(&error))?;
                if entry.header().entry_type().is_file()
                    && named(&entry.path().map_err(|error| damaged(&error))?, runtime.file)
                {
                    std::io::copy(&mut entry, &mut out).map_err(unwritable)?;
                    copied = true;
                    break;
                }
            }
        }
        Archive::Zip => {
            let mut zip = zip::ZipArchive::new(opened).map_err(|error| damaged(&error))?;
            for index in 0..zip.len() {
                let mut entry = zip.by_index(index).map_err(|error| damaged(&error))?;
                if entry.is_file()
                    && entry
                        .name()
                        .is_ok_and(|name| named(Path::new(name.as_ref()), runtime.file))
                {
                    std::io::copy(&mut entry, &mut out).map_err(unwritable)?;
                    copied = true;
                    break;
                }
            }
        }
    }
    if copied {
        Ok(())
    } else {
        Err(format!("the archive holds no {} in its lib folder", runtime.file))
    }
}

/// Whether `entry` is the file `file` in a `lib` folder.
fn named(entry: &Path, file: &str) -> bool {
    entry.file_name().is_some_and(|name| name == file)
        && entry
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|folder| folder == "lib")
}
