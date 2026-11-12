//! Where downloaded files are kept, where the runtime library comes from, and whether a missing piece may be downloaded.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::Error;

/// The folder name under the platform's cache folder.
const APP: &str = "bt-audio";

/// Where the model runtime finds its pieces. The default downloads nothing, uses the platform's cache folder and the
/// library installed there.
#[derive(Debug, Clone, Default)]
pub struct Options {
    cache: Option<PathBuf>,
    library: Option<PathBuf>,
    allow_download: bool,
}

impl Options {
    /// The defaults: the platform's cache folder ([`cache_dir`]), the library kept there, no downloading.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The options the environment asks for: `BT_AUDIO_ONNX_LIB` (a library file) and `BT_AUDIO_ALLOW_DOWNLOAD` (`1`,
    /// `true` or `yes`, in any case, allows downloading what is missing). An empty variable counts as unset.
    #[must_use]
    pub fn from_env() -> Self {
        Self::from_lookup(|name| std::env::var_os(name))
    }

    fn from_lookup(var: impl Fn(&str) -> Option<OsString>) -> Self {
        let var = |name: &str| var(name).filter(|value| !value.is_empty());
        Self {
            cache: None,
            library: var("BT_AUDIO_ONNX_LIB").map(PathBuf::from),
            allow_download: var("BT_AUDIO_ALLOW_DOWNLOAD")
                .is_some_and(|value| matches!(value.to_string_lossy().to_lowercase().as_str(), "1" | "true" | "yes")),
        }
    }

    /// These options keeping downloads in `folder` instead of [`cache_dir`].
    #[must_use]
    pub fn with_cache(mut self, folder: impl Into<PathBuf>) -> Self {
        self.cache = Some(folder.into());
        self
    }

    /// These options loading ONNX Runtime from `file` (a `.so`, `.dylib` or `.dll`) instead of the installed one.
    #[must_use]
    pub fn with_library(mut self, file: impl Into<PathBuf>) -> Self {
        self.library = Some(file.into());
        self
    }

    /// These options allowing (or not) the download of what is missing when something asks for it.
    #[must_use]
    pub const fn allow_download(mut self, allow: bool) -> Self {
        self.allow_download = allow;
        self
    }

    /// Whether downloading what is missing is allowed.
    #[must_use]
    pub const fn downloads(&self) -> bool {
        self.allow_download
    }

    /// The library file these options name, if any.
    #[must_use]
    pub fn library(&self) -> Option<&Path> {
        self.library.as_deref()
    }

    /// The folder these options keep downloads in.
    ///
    /// # Errors
    /// [`Error::ModelUnavailable`] naming `HOME` when no cache folder was given and the platform has none.
    pub fn cache(&self) -> Result<PathBuf, Error> {
        self.cache.clone().map_or_else(cache_dir, Ok)
    }
}

/// The folder downloaded files are kept in: `bt-audio` in the platform's cache folder (macOS `~/Library/Caches`, Windows
/// `%LOCALAPPDATA%`, elsewhere `$XDG_CACHE_HOME` or `~/.cache`).
///
/// # Errors
/// [`Error::ModelUnavailable`] naming `HOME` when the platform has no cache folder for this user.
pub fn cache_dir() -> Result<PathBuf, Error> {
    under_app(dirs::cache_dir())
}

fn under_app(base: Option<PathBuf>) -> Result<PathBuf, Error> {
    base.map(|base| base.join(APP)).ok_or_else(|| {
        Error::ModelUnavailable(
            "the cache folder is not known (no home folder): set HOME, or give a cache folder".to_owned(),
        )
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn lookup(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let vars: HashMap<String, OsString> = pairs.iter().map(|(k, v)| ((*k).to_owned(), (*v).into())).collect();
        move |name| vars.get(name).cloned()
    }

    #[test]
    fn the_cache_is_bt_audio_under_the_platform_folder() {
        assert_eq!(under_app(Some(PathBuf::from("/c"))), Ok(PathBuf::from("/c/bt-audio")));
    }

    #[test]
    fn no_platform_folder_is_refused_naming_home() {
        assert!(matches!(under_app(None), Err(Error::ModelUnavailable(message)) if message.contains("HOME")));
    }

    #[test]
    fn downloading_is_allowed_only_by_a_true_word() {
        for (value, allowed) in [
            ("1", true),
            ("true", true),
            ("YES", true),
            ("Yes", true),
            ("0", false),
            ("no", false),
            ("", false),
            ("2", false),
        ] {
            let options = Options::from_lookup(lookup(&[("BT_AUDIO_ALLOW_DOWNLOAD", value)]));
            assert_eq!(options.downloads(), allowed, "{value:?}");
        }
        assert!(!Options::from_lookup(lookup(&[])).downloads(), "unset");
    }

    #[test]
    fn the_library_comes_from_bt_audio_onnx_lib_and_an_empty_one_is_unset() {
        let named = Options::from_lookup(lookup(&[("BT_AUDIO_ONNX_LIB", "/x/lib.so")]));
        assert_eq!(named.library(), Some(Path::new("/x/lib.so")));
        assert_eq!(
            Options::from_lookup(lookup(&[("BT_AUDIO_ONNX_LIB", "")])).library(),
            None
        );
        assert_eq!(
            Options::from_lookup(lookup(&[("BT_IMAGE_ONNX_LIB", "/x")])).library(),
            None
        );
    }
}
