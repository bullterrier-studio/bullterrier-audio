//! The model runtime through the public API: downloads against a server on this computer, archives built in memory,
//! where things are kept and found, and the stand-in model. No test here needs the network or a model; the one that needs
//! the installed runtime and model is `#[ignore]`d and run by `poe rust-test-models`.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::thread;

use bullterrier_audio::models::{
    Archive, Model, ModelKind, Options, OrtInfer, Runtime, Status, fetch, install_model, install_runtime_release,
    load_runtime, model_file, model_status, models, runtime_library, runtime_status, runtimes,
};
use bullterrier_audio::{Error, Infer, Tensor};
use sha2::{Digest, Sha256};

use crate::common::Outcome;

/// The SHA-256 of "abc" (FIPS 180-4's own example): the file most rows ask for.
const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

/// A server on this computer that answers each request with the next of `answers` (raw HTTP), then stops. Its base URL.
fn serve(answers: Vec<Vec<u8>>) -> Result<String, std::io::Error> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let base = format!("http://{}", listener.local_addr()?);
    thread::spawn(move || {
        for answer in answers {
            let Ok((mut stream, _)) = listener.accept() else { return };
            let mut request = [0_u8; 4096];
            drop(stream.read(&mut request));
            drop(stream.write_all(&answer));
        }
    });
    Ok(base)
}

fn ok(body: &[u8]) -> Vec<u8> {
    let mut answer = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    answer.extend_from_slice(body);
    answer
}

fn raw(text: &str) -> Vec<u8> {
    text.as_bytes().to_vec()
}

fn names(folder: &Path) -> Result<Vec<String>, std::io::Error> {
    let mut found: Vec<String> = std::fs::read_dir(folder)?
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .collect();
    found.sort();
    Ok(found)
}

/// Whether `result` is a `Write` error for `path` whose reason holds every word.
fn refused_with(result: &Result<(), Error>, path: &Path, words: &[&str]) -> bool {
    matches!(result, Err(Error::Write { path: at, reason }) if at == path && words.iter().all(|word| reason.contains(word)))
}

/// A model whose file is `bytes`, served at `url`.
fn model(url: &str, bytes: &[u8]) -> Model {
    Model {
        name: "stand-in",
        url: Box::leak(url.to_owned().into_boxed_str()),
        sha256: Box::leak(format!("{:x}", Sha256::digest(bytes)).into_boxed_str()),
        size: bytes.len() as u64,
        licence: "MIT",
        kind: ModelKind::Notes,
    }
}

mod download {
    use super::*;

    #[test]
    fn a_file_with_the_right_size_and_checksum_arrives_and_nothing_else_stays() -> Outcome {
        let (base, dir) = (serve(vec![ok(b"abc")])?, tempfile::tempdir()?);
        let target = dir.path().join("deep").join("m.onnx");
        fetch(&format!("{base}/m.onnx"), ABC, 3, &target)?;
        assert_eq!(std::fs::read(&target)?, b"abc");
        assert_eq!(
            names(&dir.path().join("deep"))?,
            ["m.onnx"],
            "no temporary file beside it"
        );
        Ok(())
    }

    #[test]
    fn a_file_that_verifies_is_used_without_the_network() -> Outcome {
        let dir = tempfile::tempdir()?;
        std::fs::write(dir.path().join("m.onnx"), b"abc")?;
        fetch("http://127.0.0.1:9/never", ABC, 3, &dir.path().join("m.onnx"))?; // nothing listens there
        Ok(())
    }

    #[test]
    fn a_broken_copy_is_replaced() -> Outcome {
        let (base, dir) = (serve(vec![ok(b"abc")])?, tempfile::tempdir()?);
        std::fs::write(dir.path().join("m.onnx"), b"xyz")?;
        fetch(&format!("{base}/m.onnx"), ABC, 3, &dir.path().join("m.onnx"))?;
        assert_eq!(std::fs::read(dir.path().join("m.onnx"))?, b"abc");
        Ok(())
    }

    #[test]
    fn refusals_name_the_url_and_the_target_and_leave_nothing() -> Outcome {
        let to_http = "HTTP/1.1 302 Found\r\nLocation: http://example.com/m.onnx\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
        let truncated = "HTTP/1.1 200 OK\r\nContent-Length: 3\r\nConnection: close\r\n\r\nab";
        let unsaid = "HTTP/1.1 200 OK\r\nConnection: close\r\n\r\nabcd";
        let not_found = "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
        let short = "HTTP/1.1 200 OK\r\nConnection: close\r\n\r\nab";
        let announced = "HTTP/1.1 200 OK\r\nContent-Length: 99\r\nConnection: close\r\n\r\nabc";
        let rows: [(&str, Vec<u8>, &[&str]); 8] = [
            ("another checksum", ok(b"abd"), &["SHA-256", ABC, "was not saved"]),
            (
                "a larger Content-Length than the size",
                ok(b"abcd"),
                &["larger than the expected 3 bytes"],
            ),
            (
                "a Content-Length over the size, refused before the body is read",
                raw(announced),
                &["larger than the expected 3 bytes"],
            ),
            (
                "a body longer than the size, none announced",
                raw(unsaid),
                &["larger than the expected 3 bytes"],
            ),
            (
                "a body that ends early, none announced",
                raw(short),
                &["stopped after 2 of 3 bytes"],
            ),
            (
                "a connection that drops mid-body",
                raw(truncated),
                &["could not download"],
            ),
            ("a 404", raw(not_found), &["404"]),
            (
                "a redirect to plain http elsewhere",
                raw(to_http),
                &["redirected to http://example.com/m.onnx", "not allowed"],
            ),
        ];
        for (rule, answer, words) in rows {
            let (base, dir) = (serve(vec![answer])?, tempfile::tempdir()?);
            let (url, target) = (format!("{base}/m.onnx"), dir.path().join("m.onnx"));
            let refused = fetch(&url, ABC, 3, &target);
            let mut expected = vec![url.as_str(), "by hand", "m.onnx"];
            expected.extend_from_slice(words);
            assert!(refused_with(&refused, &target, &expected), "{rule}: {refused:?}");
            assert!(names(dir.path())?.is_empty(), "{rule}: left {:?}", names(dir.path())?);
        }
        Ok(())
    }

    #[test]
    fn plain_http_to_another_computer_is_refused_before_anything_is_sent() -> Outcome {
        let dir = tempfile::tempdir()?;
        let target = dir.path().join("m.onnx");
        let refused = fetch("http://example.com/m.onnx", ABC, 3, &target);
        assert!(
            refused_with(
                &refused,
                &target,
                &["only https", "by hand", "http://example.com/m.onnx"]
            ),
            "{refused:?}"
        );
        Ok(())
    }

    #[test]
    fn a_redirect_to_a_path_on_the_same_server_is_followed() -> Outcome {
        let to_path = "HTTP/1.1 302 Found\r\nLocation: /real.onnx\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
        let (base, dir) = (serve(vec![raw(to_path), ok(b"abc")])?, tempfile::tempdir()?);
        fetch(&format!("{base}/m.onnx"), ABC, 3, &dir.path().join("m.onnx"))?;
        assert_eq!(std::fs::read(dir.path().join("m.onnx"))?, b"abc");
        Ok(())
    }

    #[test]
    fn an_unwritable_folder_is_refused_naming_it() -> Outcome {
        let dir = tempfile::tempdir()?;
        std::fs::write(dir.path().join("in-the-way"), b"a file where a folder should be")?;
        let target = dir.path().join("in-the-way").join("m.onnx");
        let refused = fetch("https://example.com/m.onnx", ABC, 3, &target);
        assert!(
            refused_with(&refused, &target, &["cannot write to the folder", "in-the-way"]),
            "{refused:?}"
        );
        Ok(())
    }
}

/// A gzip tar holding `files` (path, content).
fn tarball(files: &[(&str, &[u8])]) -> Result<Vec<u8>, std::io::Error> {
    let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast()));
    for (name, content) in files {
        let mut header = tar::Header::new_gnu();
        header.set_size(content.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        builder.append_data(&mut header, name, *content)?;
    }
    builder.into_inner()?.finish()
}

/// A zip holding `files` (path, content).
fn zipped(files: &[(&str, &[u8])]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, content) in files {
        writer.start_file(*name, zip::write::SimpleFileOptions::default())?;
        writer.write_all(content)?;
    }
    Ok(writer.finish()?.into_inner())
}

/// The release of a made-up platform whose archive is `bytes`, served at `url`.
fn runtime(url: &str, bytes: &[u8], archive: Archive, file: &'static str) -> Runtime {
    Runtime {
        os: "test",
        arch: "test",
        url: Box::leak(url.to_owned().into_boxed_str()),
        sha256: Box::leak(format!("{:x}", Sha256::digest(bytes)).into_boxed_str()),
        size: bytes.len() as u64,
        archive,
        file,
    }
}

mod archives {
    use super::*;

    fn kept_in(cache: &Path, file: &str) -> PathBuf {
        cache.join("onnxruntime-1.23.2").join(file)
    }

    #[test]
    fn the_library_of_a_tarball_is_taken_out_and_kept_and_the_archive_is_not() -> Outcome {
        let bytes = tarball(&[
            ("onnxruntime-x/include/onnxruntime_c_api.h", b"header"),
            ("onnxruntime-x/lib/libonnxruntime.so.1.23.2", b"THE LIBRARY"),
            ("onnxruntime-x/lib/libonnxruntime_providers_shared.so", b"other"),
        ])?;
        let release = runtime(
            &format!("{}/a", serve(vec![ok(&bytes)])?),
            &bytes,
            Archive::TarGz,
            "libonnxruntime.so.1.23.2",
        );
        let (dir, options) = options_in_tempdir()?;
        let kept = install_runtime_release(&release, &options)?;
        assert_eq!(kept, kept_in(dir.path(), "libonnxruntime.so.1.23.2"));
        assert_eq!(std::fs::read(&kept)?, b"THE LIBRARY");
        assert_eq!(
            names(kept.parent().ok_or("parent")?)?,
            ["libonnxruntime.so.1.23.2"],
            "no archive, no partial file"
        );
        // a library kept is used without touching the network: the server answered once and is gone
        assert_eq!(install_runtime_release(&release, &options)?, kept);
        Ok(())
    }

    #[test]
    fn the_library_of_a_zip_is_taken_out_and_kept() -> Outcome {
        let bytes = zipped(&[
            ("onnxruntime-x/lib/onnxruntime.dll", b"DLL"),
            ("onnxruntime-x/lib/onnxruntime.lib", b"import"),
        ])?;
        let release = runtime(
            &format!("{}/a", serve(vec![ok(&bytes)])?),
            &bytes,
            Archive::Zip,
            "onnxruntime.dll",
        );
        let (_dir, options) = options_in_tempdir()?;
        let kept = install_runtime_release(&release, &options)?;
        assert_eq!(std::fs::read(&kept)?, b"DLL");
        assert_eq!(names(kept.parent().ok_or("parent")?)?, ["onnxruntime.dll"]);
        Ok(())
    }

    #[test]
    fn archives_that_cannot_give_the_library_are_refused_naming_the_file_and_leave_nothing() -> Outcome {
        let no_lib = tarball(&[
            ("onnxruntime-x/lib/other.so", b"x"),
            ("elsewhere/libonnxruntime.so.1.23.2", b"not in lib"),
        ])?;
        let no_lib_zip = zipped(&[("onnxruntime-x/lib/other.dll", b"x")])?;
        let not_an_archive = b"this is not gzip at all".to_vec();
        let rows = [
            (
                "no entry in lib (tar)",
                no_lib,
                Archive::TarGz,
                "holds no libonnxruntime.so.1.23.2",
            ),
            (
                "no entry in lib (zip)",
                no_lib_zip,
                Archive::Zip,
                "holds no libonnxruntime.so.1.23.2",
            ),
            ("damaged archive", not_an_archive, Archive::TarGz, "damaged"),
        ];
        for (rule, bytes, archive, words) in rows {
            let release = runtime(
                &format!("{}/a", serve(vec![ok(&bytes)])?),
                &bytes,
                archive,
                "libonnxruntime.so.1.23.2",
            );
            let (dir, options) = options_in_tempdir()?;
            let refused = install_runtime_release(&release, &options);
            let target = kept_in(dir.path(), "libonnxruntime.so.1.23.2");
            assert!(
                matches!(&refused, Err(Error::Write { path, reason }) if *path == target && reason.contains(words) && reason.contains("by hand")),
                "{rule}: {refused:?}"
            );
            assert!(
                names(target.parent().ok_or("parent")?)?.is_empty(),
                "{rule}: left {:?}",
                names(target.parent().ok_or("parent")?)?
            );
        }
        Ok(())
    }

    #[test]
    fn an_archive_that_is_not_the_pinned_one_is_refused() -> Outcome {
        let bytes = tarball(&[("a/lib/libonnxruntime.so.1.23.2", b"x")])?;
        let mut release = runtime(
            &format!("{}/a", serve(vec![ok(&bytes)])?),
            &bytes,
            Archive::TarGz,
            "libonnxruntime.so.1.23.2",
        );
        release.sha256 = "0".repeat(64).leak();
        let (dir, options) = options_in_tempdir()?;
        let refused = install_runtime_release(&release, &options);
        assert!(
            matches!(&refused, Err(Error::Write { reason, .. }) if reason.contains("SHA-256")),
            "{refused:?}"
        );
        assert!(!kept_in(dir.path(), "libonnxruntime.so.1.23.2").exists());
        Ok(())
    }
}

fn options_in_tempdir() -> Result<(tempfile::TempDir, Options), std::io::Error> {
    let dir = tempfile::tempdir()?;
    let options = Options::new().with_cache(dir.path());
    Ok((dir, options))
}

mod where_things_are {
    use super::*;

    /// Where the library of this platform is kept under `cache`.
    fn kept_library(cache: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
        let here = runtimes()
            .iter()
            .find(|r| r.os == std::env::consts::OS && r.arch == std::env::consts::ARCH)
            .ok_or("no pinned runtime for this platform")?;
        Ok(cache.join("onnxruntime-1.23.2").join(here.file))
    }

    #[test]
    fn the_default_cache_is_bt_audio_in_the_platform_cache_folder() -> Outcome {
        let cache = bullterrier_audio::models::cache_dir()?;
        assert_eq!(cache.file_name().and_then(|n| n.to_str()), Some("bt-audio"));
        assert_eq!(cache, Options::new().cache()?);
        assert_eq!(
            Options::new().with_cache("/elsewhere").cache()?,
            Path::new("/elsewhere")
        );
        Ok(())
    }

    #[test]
    fn the_library_is_the_named_one_else_the_kept_one_else_an_error_that_says_how_to_get_it() -> Outcome {
        let (dir, options) = options_in_tempdir()?;
        let missing = runtime_library(&options);
        assert!(
            matches!(&missing, Err(Error::ModelUnavailable(m)) if m.contains("`bt-audio install`") && m.contains("BT_AUDIO_ALLOW_DOWNLOAD") && m.contains("BT_AUDIO_ONNX_LIB")),
            "{missing:?}"
        );
        let kept = kept_library(dir.path())?;
        assert_eq!(runtime_status(&options)?, Status::NotInstalled(kept.clone()));
        std::fs::create_dir_all(kept.parent().ok_or("parent")?)?;
        std::fs::write(&kept, b"x")?;
        assert_eq!(runtime_library(&options)?, kept);
        assert_eq!(runtime_status(&options)?, Status::Installed(kept));
        let named = options.with_library("/elsewhere/lib.so");
        assert_eq!(
            runtime_library(&named)?,
            Path::new("/elsewhere/lib.so"),
            "the named library wins over the kept one"
        );
        assert_eq!(
            runtime_status(&named)?,
            Status::NotInstalled("/elsewhere/lib.so".into())
        );
        Ok(())
    }

    #[test]
    fn a_missing_model_is_an_error_that_names_the_fix_unless_downloading_is_allowed() -> Outcome {
        let (dir, options) = options_in_tempdir()?;
        let missing = model_file(&models()[0], &options);
        assert!(
            matches!(&missing, Err(Error::ModelUnavailable(m)) if m.contains("`bt-audio install`") && m.contains("basic-pitch") && m.contains("BT_AUDIO_ALLOW_DOWNLOAD=1")),
            "{missing:?}"
        );
        let bytes = b"abc";
        let stand_in = model(&format!("{}/m", serve(vec![ok(bytes)])?), bytes);
        let file = model_file(&stand_in, &options.allow_download(true))?;
        assert_eq!(file, dir.path().join("stand-in.onnx"));
        assert_eq!(std::fs::read(file)?, bytes);
        Ok(())
    }

    #[test]
    fn a_model_is_installed_not_installed_or_damaged_and_a_damaged_one_is_not_used() -> Outcome {
        let (dir, options) = options_in_tempdir()?;
        let bytes = b"model bytes";
        let stand_in = model(&format!("{}/m", serve(vec![ok(bytes)])?), bytes);
        let file = dir.path().join("stand-in.onnx");
        assert_eq!(model_status(&stand_in, &options)?, Status::NotInstalled(file.clone()));
        assert_eq!(install_model(&stand_in, &options)?, file);
        assert_eq!(model_status(&stand_in, &options)?, Status::Installed(file.clone()));
        let mut corrupt = bytes.to_vec();
        corrupt[3] ^= 1; // one byte, same size
        std::fs::write(&file, corrupt)?;
        assert!(
            matches!(model_status(&stand_in, &options)?, Status::Damaged { path, reason } if path == file && reason.contains("SHA-256"))
        );
        let refused = model_file(&stand_in, &options);
        assert!(
            matches!(&refused, Err(Error::ModelUnavailable(m)) if m.contains("bt-audio install")),
            "{refused:?}"
        );
        Ok(())
    }

    #[test]
    fn the_catalogue_pins_basic_pitch() {
        let [pitch] = models() else { unreachable!("one model") };
        assert_eq!(
            (pitch.name, pitch.licence, pitch.kind),
            ("basic-pitch", "Apache-2.0", ModelKind::Notes)
        );
        assert!(pitch.url.starts_with("https://") && pitch.url.contains("v0.4.0"));
        assert_eq!(pitch.sha256.len(), 64);
    }

    #[test]
    fn every_platform_has_one_pinned_release_with_a_checksum_and_a_library_file() {
        assert_eq!(runtimes().len(), 6);
        for release in runtimes() {
            assert_eq!(release.sha256.len(), 64, "{release:?}");
            assert!(
                release.url.starts_with("https://") && release.url.contains("1.23.2"),
                "{release:?}"
            );
            let expected = if release.os == "windows" {
                Archive::Zip
            } else {
                Archive::TarGz
            };
            assert_eq!(release.archive, expected, "{release:?}");
        }
    }

    #[test]
    fn a_file_that_is_not_a_library_is_installed_but_cannot_be_loaded() -> Outcome {
        let dir = tempfile::tempdir()?;
        let junk = dir.path().join("junk.dylib");
        std::fs::write(&junk, b"not a library")?;
        let options = Options::new().with_cache(dir.path()).with_library(&junk);
        assert_eq!(runtime_status(&options)?, Status::Installed(junk));
        assert!(matches!(load_runtime(&options), Err(Error::ModelUnavailable(m)) if m.contains("junk.dylib")));
        let missing = Options::new()
            .with_cache(dir.path())
            .with_library(dir.path().join("gone.dylib"));
        assert!(matches!(load_runtime(&missing), Err(Error::ModelUnavailable(m)) if m.contains("gone.dylib")));
        Ok(())
    }

    #[test]
    fn a_library_that_is_not_there_cannot_be_loaded() -> Outcome {
        let dir = tempfile::tempdir()?;
        let loaded = OrtInfer::from_file(&dir.path().join("no-lib.dylib"), &dir.path().join("m.onnx"));
        assert!(matches!(loaded, Err(Error::ModelUnavailable(m)) if m.contains("no-lib.dylib")));
        Ok(())
    }
}

mod tensors {
    use super::*;

    #[test]
    fn the_shape_must_multiply_out_to_the_number_of_values() {
        let rows: [(&[usize], usize, bool); 6] = [
            (&[2, 3], 6, true),
            (&[1, 4, 1], 4, true),
            (&[], 1, true),
            (&[2, 3], 5, false),
            (&[0, 3], 1, false),
            (&[usize::MAX, 2], 2, false),
        ];
        for (shape, count, valid) in rows {
            let made = Tensor::new(shape.to_vec(), vec![0.0; count]);
            assert_eq!(made.is_ok(), valid, "shape {shape:?} with {count} values");
            if let Err(error) = made {
                assert!(matches!(error, Error::Invalid(m) if m.contains("shape")));
            }
        }
    }

    /// A model that answers with its outputs in a fixed order: the input's sum, then the input doubled.
    struct StandIn;

    impl Infer for StandIn {
        fn infer(&self, input: &Tensor) -> Result<Vec<Tensor>, Error> {
            let sum: f32 = input.values().iter().sum();
            Ok(vec![
                Tensor::new(vec![1], vec![sum])?,
                Tensor::new(input.shape().to_vec(), input.values().iter().map(|v| v * 2.0).collect())?,
            ])
        }
    }

    #[test]
    fn a_model_gives_every_output_in_its_own_order() -> Outcome {
        let model: Box<dyn Infer> = Box::new(StandIn);
        let outputs = model.infer(&Tensor::new(vec![1, 3], vec![1.0, 2.0, 3.0])?)?;
        let shapes: Vec<&[usize]> = outputs.iter().map(Tensor::shape).collect();
        assert_eq!(shapes, [&[1][..], &[1, 3][..]]);
        assert_eq!(outputs[0].values(), [6.0]);
        assert_eq!(outputs[1].values(), [2.0, 4.0, 6.0]);
        Ok(())
    }
}

/// The installed runtime and model, by the default options (the developer's cache folder, or `BT_AUDIO_ONNX_LIB`): the
/// verified input and outputs of Basic Pitch.
#[test]
#[ignore = "needs the installed runtime and model: poe rust-test-models"]
fn the_real_basic_pitch_has_the_verified_input_and_outputs() -> Outcome {
    let infer = OrtInfer::open(&models()[0], &Options::from_env())?;
    assert_eq!(infer.input_name(), "serving_default_input_2:0");
    assert_eq!(
        infer.output_names(),
        [
            "StatefulPartitionedCall:2",
            "StatefulPartitionedCall:1",
            "StatefulPartitionedCall:0"
        ]
    );
    let outputs = infer.infer(&Tensor::new(vec![1, 43844, 1], vec![0.0; 43844])?)?;
    let shapes: Vec<&[usize]> = outputs.iter().map(Tensor::shape).collect();
    assert_eq!(shapes, [&[1, 172, 88][..], &[1, 172, 88][..], &[1, 172, 264][..]]);
    assert!(
        outputs
            .iter()
            .all(|o| o.values().iter().all(|v| (0.0..=1.0).contains(v))),
        "posteriors are in 0..1"
    );
    Ok(())
}

/// ONNX Runtime allows one library per process, but a library that failed to load must not be remembered: the next call
/// with a good one works.
#[test]
#[ignore = "needs the installed runtime and model: poe rust-test-models"]
fn a_library_that_failed_to_load_does_not_stop_a_good_one_later() -> Outcome {
    let dir = tempfile::tempdir()?;
    let options = Options::from_env();
    let model = model_file(&models()[0], &options)?;
    let failed = OrtInfer::from_file(&dir.path().join("no-lib.dylib"), &model);
    assert!(matches!(failed, Err(Error::ModelUnavailable(m)) if m.contains("no-lib.dylib")));
    let opened = OrtInfer::open(&models()[0], &options)?;
    assert_eq!(opened.output_names().len(), 3);
    Ok(())
}

/// Loading takes a moment, so the session of a model is loaded once per process and every caller gets it.
#[test]
#[ignore = "needs the installed runtime and model: poe rust-test-models"]
fn opening_a_model_again_gives_the_session_already_loaded() -> Outcome {
    let options = Options::from_env();
    let first = OrtInfer::open(&models()[0], &options)?;
    assert!(first == OrtInfer::open(&models()[0], &options)?);
    let own = OrtInfer::from_file(&runtime_library(&options)?, &model_file(&models()[0], &options)?)?;
    assert!(first != own, "from_file always loads a session of its own");
    Ok(())
}

#[test]
#[ignore = "needs the installed runtime: poe rust-test-models"]
fn the_installed_runtime_loads_and_reports_its_build() -> Outcome {
    let info = load_runtime(&Options::from_env())?;
    assert!(info.contains("git-commit-id="), "{info}");
    Ok(())
}
