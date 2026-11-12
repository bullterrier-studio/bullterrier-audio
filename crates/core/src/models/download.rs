//! Getting a file onto this computer: downloaded once, verified, and kept.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use sha2::{Digest, Sha256};
use ureq::http::Uri;

use crate::Error;

/// How long a stalled connection may stay silent before the download fails.
const SILENCE: Duration = Duration::from_secs(60); // constant: a download that is quiet this long is dead
/// How many redirects are followed, each checked.
const REDIRECTS: usize = 8; // constant: more than any release host chains
/// The age after which a temporary file of an earlier download is surely dead (a killed process, a power cut).
const DEAD_AFTER: Duration = Duration::from_hours(24); // constant: a younger one may be another process downloading
/// Bytes read at a time: memory use only, the result does not depend on it.
const CHUNK: usize = 1 << 20; // constant: 1 MiB

/// `url` to `target`, unless `target` already holds a file of `size` bytes with SHA-256 `sha256` (lowercase hex). Missing
/// folders are created; a file at `target` that does not verify (an earlier broken copy) is replaced.
///
/// Promises that `target` only ever holds a verified file, and that no partial file stays beside it, whatever goes wrong.
/// `url` is `https://`, or `http://` to this computer (`127.0.0.1`, `[::1]`, `localhost`); a redirect may lead only to
/// the same. A server that offers or sends more than `size` bytes is cut off at once, so it cannot fill the disk.
///
/// # Errors
/// [`Error::Write`] for `target`, with a reason that names `url` and says to download it by hand, when the folder cannot
/// be written, the download fails, the URL or a redirect is not allowed, or what arrived has another size or digest.
/// `target` is then left as it was and no temporary file remains.
pub fn fetch(url: &str, sha256: &str, size: u64, target: &Path) -> Result<(), Error> {
    let by_hand = format!("download {url} by hand and save it as {}", target.display());
    let fail = |reason: String| Error::Write {
        path: target.to_path_buf(),
        reason: format!("{reason}; {by_hand}"),
    };
    if verifies(target, sha256, size) {
        return Ok(());
    }
    allowed(url).map_err(|why| fail(format!("{why}: {url}")))?;
    let (Some(folder), Some(name)) = (target.parent(), target.file_name().and_then(|name| name.to_str())) else {
        return Err(fail("it is no file path".to_owned()));
    };
    let temporary = folder.join(format!(".{name}.{}.part", unique()));
    let opened = std::fs::create_dir_all(folder).and_then(|()| {
        clear_dead(folder, name);
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
    });
    let mut file =
        opened.map_err(|error| fail(format!("cannot write to the folder {} ({error})", folder.display())))?;
    let outcome = transfer(url, sha256, size, &mut file).map_err(fail);
    drop(file); // closed before the rename, which Windows requires
    let outcome = outcome.and_then(|()| {
        std::fs::rename(&temporary, target).map_err(|error| fail(format!("cannot save {} ({error})", target.display())))
    });
    if outcome.is_err() {
        // best effort: a temporary that cannot be removed is cleared as dead after a day
        drop(std::fs::remove_file(&temporary));
    }
    outcome
}

/// Whether `path` is a file of `size` bytes with SHA-256 `sha256`.
pub(super) fn verifies(path: &Path, sha256: &str, size: u64) -> bool {
    std::fs::metadata(path).is_ok_and(|meta| meta.is_file() && meta.len() == size)
        && sha256_of(path).is_ok_and(|found| found == sha256)
}

/// The SHA-256 of the file at `path`, as 64 lowercase hex digits.
fn sha256_of(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut digest = Sha256::new();
    let mut chunk = vec![0_u8; CHUNK];
    loop {
        let read = file.read(&mut chunk)?;
        if read == 0 {
            return Ok(format!("{:x}", digest.finalize()));
        }
        digest.update(&chunk[..read]);
    }
}

/// `Ok` for https anywhere or http to this computer, else why not.
fn allowed(url: &str) -> Result<(), String> {
    let uri: Uri = url.parse().map_err(|error| format!("{url} is not a URL ({error})"))?;
    match (uri.scheme_str(), uri.host()) {
        (Some("https"), Some(_)) | (Some("http"), Some("127.0.0.1" | "[::1]" | "localhost")) => Ok(()),
        (Some(scheme), _) => Err(format!(
            "only https downloads are allowed from other computers, not {scheme}"
        )),
        (None, _) => Err("a download needs a full URL".to_owned()),
    }
}

/// `url` copied into `file` (following allowed redirects), checked against `size` and `sha256` as it arrives; the reason
/// when it fails.
fn transfer(url: &str, sha256: &str, size: u64, file: &mut std::fs::File) -> Result<(), String> {
    let agent = ureq::Agent::config_builder()
        .max_redirects(0)
        .http_status_as_error(false)
        .timeout_per_read(Some(SILENCE))
        .build()
        .new_agent();
    let mut here = url.to_owned();
    for _ in 0..=REDIRECTS {
        let response = agent
            .get(&here)
            .call()
            .map_err(|error| format!("could not download {here} ({error})"))?;
        let status = response.status().as_u16();
        if (300..400).contains(&status) {
            let to = response
                .headers()
                .get("location")
                .and_then(|value| value.to_str().ok())
                .ok_or_else(|| format!("{here} redirected without saying where"))?;
            let next = resolve(&here, to);
            allowed(&next).map_err(|why| format!("redirected to {next}, which is not allowed ({why})"))?;
            here = next;
            continue;
        }
        if status != 200 {
            return Err(format!("{here} answered {status}, not the file"));
        }
        return copy(response, file, (url, sha256, size));
    }
    Err(format!("{url} redirected more than {REDIRECTS} times"))
}

/// The body of `response` into `file`, at most `size` bytes, verified.
fn copy(
    mut response: ureq::http::Response<ureq::Body>,
    file: &mut std::fs::File,
    (url, sha256, size): (&str, &str, u64),
) -> Result<(), String> {
    let too_large = || format!("what {url} sends is larger than the expected {size} bytes, so it is not the file");
    let offered = response
        .headers()
        .get("content-length")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok());
    if offered.is_some_and(|offered| offered > size) {
        return Err(too_large());
    }
    let mut reader = response.body_mut().as_reader();
    let (mut digest, mut received) = (Sha256::new(), 0_u64);
    let mut chunk = vec![0_u8; CHUNK];
    loop {
        let read = reader
            .read(&mut chunk)
            .map_err(|error| format!("could not download {url} ({error})"))?;
        if read == 0 {
            break;
        }
        received += read as u64;
        if received > size {
            return Err(too_large());
        }
        file.write_all(&chunk[..read])
            .map_err(|error| format!("cannot write the download of {url} ({error})"))?;
        digest.update(&chunk[..read]);
    }
    if received != size {
        return Err(format!(
            "the download of {url} stopped after {received} of {size} bytes"
        ));
    }
    let found = format!("{:x}", digest.finalize());
    if found != sha256 {
        return Err(format!(
            "the file downloaded from {url} has SHA-256 {found}, but {sha256} was expected, so it was not saved"
        ));
    }
    Ok(())
}

/// `location` as an absolute URL, given the URL `here` it came from.
fn resolve(here: &str, location: &str) -> String {
    if location.starts_with('/')
        && let Ok(uri) = here.parse::<Uri>()
        && let (Some(scheme), Some(authority)) = (uri.scheme_str(), uri.authority())
    {
        return format!("{scheme}://{authority}{location}");
    }
    location.to_owned()
}

/// A name no other download of this process or another shares.
fn unique() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    format!(
        "{:x}{:x}{:x}",
        std::process::id(),
        nanos,
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

/// Deletes the temporary files of earlier downloads of `name` in `folder` that died without cleaning up.
fn clear_dead(folder: &Path, name: &str) {
    let Ok(entries) = std::fs::read_dir(folder) else { return };
    let prefix = format!(".{name}.");
    for entry in entries.flatten() {
        let path: PathBuf = entry.path();
        let leftover = path.file_name().and_then(|file| file.to_str()).is_some_and(|file| {
            file.starts_with(&prefix) && Path::new(file).extension().is_some_and(|ext| ext == "part")
        });
        let dead = entry
            .metadata()
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|modified| SystemTime::now().duration_since(modified).ok())
            .is_some_and(|age| age > DEAD_AFTER);
        if leftover && dead {
            // not ours to force: one held open by its process (Windows) or already gone stays for the next time
            drop(std::fs::remove_file(&path));
        }
    }
}
