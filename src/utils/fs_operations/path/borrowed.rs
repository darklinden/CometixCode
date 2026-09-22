use super::cwd::try_get_current_dir;
use super::normalize::{
    TrailingSeparator, normalize_for_resolution, normalize_owned_for_resolution, normalize_path,
};
use super::relative::{RelativeOutcome, relative_outcome_with, try_relative_outcome};
use super::slash::replace_main_separator;
use std::borrow::Cow;
use std::io;
use std::path::{Path, PathBuf};

mod private {
    use std::path::Path;

    pub trait Sealed {}

    impl Sealed for Path {}
    impl Sealed for str {}
}

/// Lexical path operations over borrowed standard Rust path and string types.
///
/// Import this trait to call its methods on [`Path`] and `str`. [`PathBuf`],
/// [`String`], and other types that dereference to one of those types use the
/// methods through normal method lookup; they do not implement `SugarPath`
/// themselves.
///
/// The trait is sealed because it is an extension-method namespace, not an
/// abstraction for downstream path types. Generic APIs should accept a
/// standard bound such as [`AsRef<Path>`], then call SugarPath methods on the
/// resulting `&Path`.
///
/// Methods returning [`Cow`] may borrow from the receiver when its existing
/// storage already contains the result. They never borrow from a `base` or
/// `cwd` argument.
///
/// # Generic code
///
/// ```ignore
/// use std::path::{Path, PathBuf};
/// use sugar_path::SugarPath;
///
/// fn normalized(path: impl AsRef<Path>) -> PathBuf {
///   path.as_ref().normalize().into_owned()
/// }
///
/// assert_eq!(normalized(PathBuf::from("src").join("..")), PathBuf::from("."));
/// ```
pub(crate) trait SugarPath: private::Sealed {
    /// Lexically normalizes this path in host-native syntax.
    ///
    /// This removes `.` components and redundant separators, resolves `..`
    /// against preceding normal components, and prevents a rooted path from
    /// ascending above its root. An empty path normalizes to `.`. This operation
    /// does not access the filesystem or resolve symlinks; use
    /// [`std::fs::canonicalize`] when physical filesystem identity is required.
    ///
    /// One trailing separator is preserved when the input has one. The returned
    /// [`Cow`] borrows an already-normalized receiver when possible. A canonical
    /// current-directory result may borrow the static `.` path; other results
    /// that require a new buffer are owned.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use std::path::{Path, PathBuf};
    /// use sugar_path::SugarPath;
    ///
    /// let input = PathBuf::from("workspace").join("src").join("..").join("dist");
    /// let expected = Path::new("workspace").join("dist");
    /// assert_eq!(&*input.normalize(), expected);
    /// ```
    ///
    /// # Windows
    ///
    /// Non-verbatim separators are written as `\`, and the input spelling of a
    /// drive letter is preserved. Verbatim paths retain Rust's native rule that
    /// `/` is a literal character rather than a separator. The minimal `.\` is
    /// kept or inserted when its absence would reinterpret the first normal
    /// component as a prefix.
    fn normalize(&self) -> Cow<'_, Path>;

    /// Resolves this path against the process current directory and normalizes it.
    ///
    /// Resolution removes a non-root trailing separator. An absolute input is
    /// normalized without reading or initializing process cwd state. Other
    /// inputs use the live process current directory; this internal module
    /// does not enable the upstream current-directory cache.
    ///
    /// A clean absolute receiver may be returned borrowed. A result that requires
    /// cwd resolution is owned.
    ///
    /// Prefer [`SugarPath::absolutize_with`] when the base directory is known, so
    /// the call does not depend on process cwd.
    ///
    /// # Examples
    ///
    /// Absolute inputs do not consult cwd:
    ///
    /// ```ignore
    /// use std::path::Path;
    /// use sugar_path::SugarPath;
    ///
    /// #[cfg(target_family = "unix")]
    /// assert_eq!(&*"/workspace/src".absolutize(), Path::new("/workspace/src"));
    ///
    /// #[cfg(target_family = "windows")]
    /// assert_eq!(&*r"C:\workspace\src".absolutize(), Path::new(r"C:\workspace\src"));
    /// ```
    ///
    /// # Windows
    ///
    /// On Windows, drive-relative inputs such as `C:foo` use Windows' remembered
    /// current directory for that drive. This lookup is authoritative and is not
    /// replaced by the crate's single cached cwd.
    ///
    /// # Panics
    ///
    /// Panics if required process cwd or Windows drive-cwd state cannot be
    /// resolved. Use [`SugarPath::try_absolutize`] to handle the error.
    fn absolutize(&self) -> Cow<'_, Path>;

    /// Fallible form of [`SugarPath::absolutize`].
    ///
    /// Absolute and otherwise cwd-independent inputs succeed without reading
    /// process cwd state, so they do not fail merely because ambient cwd is
    /// unavailable.
    ///
    /// # Errors
    ///
    /// Returns the underlying [`io::Error`] if required ambient cwd state cannot
    /// be obtained or a Windows drive-relative path cannot be made absolute.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use std::path::Path;
    /// use sugar_path::SugarPath;
    ///
    /// #[cfg(target_family = "unix")]
    /// assert_eq!(&*"/workspace".try_absolutize().unwrap(), Path::new("/workspace"));
    ///
    /// #[cfg(target_family = "windows")]
    /// assert_eq!(&*r"C:\workspace".try_absolutize().unwrap(), Path::new(r"C:\workspace"));
    /// ```
    fn try_absolutize(&self) -> io::Result<Cow<'_, Path>>;

    /// Resolves this path against an explicit current directory and normalizes it.
    ///
    /// This method never reads process cwd state. An absolute receiver ignores
    /// `cwd` and may be returned borrowed. A relative result that uses `cwd` is
    /// owned; an owned [`PathBuf`] passed as `cwd` may provide that result buffer.
    /// The returned value never borrows from `cwd`.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use std::path::Path;
    /// use sugar_path::SugarPath;
    ///
    /// #[cfg(target_family = "unix")]
    /// assert_eq!("src/lib.rs".absolutize_with("/workspace"), Path::new("/workspace/src/lib.rs"));
    ///
    /// #[cfg(target_family = "windows")]
    /// assert_eq!(r"src\lib.rs".absolutize_with(r"C:\workspace"), Path::new(r"C:\workspace\src\lib.rs"));
    /// ```
    ///
    /// # Windows
    ///
    /// On Windows, an ordinary relative path uses `cwd`, and a root-relative path
    /// uses `cwd`'s drive or prefix. A drive-relative receiver such as `C:foo` is
    /// resolved when `cwd` supplies drive C's context. An ordinary cwd on another
    /// drive falls back to C's root, following Node's drive-context selection.
    /// This explicit-cwd extension never reads per-drive environment variables;
    /// namespace cwd follows the same lexical resolver rules.
    ///
    /// # Panics
    ///
    /// Panics if the non-absolute receiver needs `cwd` and `cwd` is not absolute.
    /// An absolute receiver does not inspect or validate `cwd`.
    fn absolutize_with(&self, cwd: impl AsRef<Path> + Into<PathBuf>) -> Cow<'_, Path>;

    /// Returns the lexical path from `base` to this receiver.
    ///
    /// Call this as `target.relative(base)`. Both inputs are resolved as
    /// [`SugarPath::absolutize`] would resolve them, except that cwd-independent
    /// inputs avoid reading ambient cwd state. Equal resolved paths return an
    /// empty path, and result spelling never preserves a non-root target trailing
    /// separator.
    ///
    /// A result already present in the receiver, commonly a descendant suffix
    /// with trailing separators excluded, may be borrowed. Results that must be
    /// rebuilt, including upward and differently rooted results, are owned.
    ///
    /// # Windows
    ///
    /// Drive and path components compare with ASCII case ignored.
    /// Different drive, UNC share, or namespace roots return the normalized
    /// absolute target because a relative path cannot cross those roots. The
    /// normalized target is also returned when its remaining components cannot
    /// be represented by a standalone native relative [`Path`], including a
    /// verbatim component containing literal `/` or a leading component that
    /// would be reparsed as a Windows prefix. This target is normally absolute
    /// after resolution, but can remain root-relative or drive-relative when the
    /// unknown shared context deliberately cancels.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use std::path::Path;
    /// use sugar_path::SugarPath;
    ///
    /// assert_eq!(Path::new("workspace/src").relative("workspace"), Path::new("src"));
    /// ```
    ///
    /// # Panics
    ///
    /// Panics if required process cwd or Windows drive-cwd state cannot be
    /// resolved. Use [`SugarPath::try_relative`] to handle the error.
    fn relative(&self, base: impl AsRef<Path>) -> Cow<'_, Path>;

    /// Fallible form of [`SugarPath::relative`].
    ///
    /// # Errors
    ///
    /// Returns the underlying [`io::Error`] if either input requires ambient cwd
    /// state that cannot be obtained. Cwd-independent inputs do not produce this
    /// error merely because process cwd is unavailable.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use std::path::Path;
    /// use sugar_path::SugarPath;
    ///
    /// #[cfg(target_family = "unix")]
    /// {
    ///   let relative = Path::new("/workspace/src").try_relative("/workspace").unwrap();
    ///   assert_eq!(&*relative, Path::new("src"));
    /// }
    ///
    /// #[cfg(target_family = "windows")]
    /// {
    ///   let relative = Path::new(r"C:\workspace\src").try_relative(r"C:\workspace").unwrap();
    ///   assert_eq!(&*relative, Path::new("src"));
    /// }
    /// ```
    fn try_relative(&self, base: impl AsRef<Path>) -> io::Result<Cow<'_, Path>>;

    /// Returns the lexical path from `base` to this receiver using `cwd` as the
    /// explicit current directory for relative inputs.
    ///
    /// This method never reads process cwd state. If the result is independent
    /// of cwd, `cwd` is neither inspected nor validated. Otherwise `cwd` resolves
    /// both inputs using [`SugarPath::absolutize_with`]. The returned value may
    /// borrow only from this receiver, never from `base` or `cwd`.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use std::path::Path;
    /// use sugar_path::SugarPath;
    ///
    /// #[cfg(target_family = "unix")]
    /// assert_eq!("src/lib.rs".relative_with("/workspace", "/workspace"), Path::new("src/lib.rs"));
    ///
    /// #[cfg(target_family = "windows")]
    /// assert_eq!(r"src\lib.rs".relative_with(r"C:\workspace", r"C:\workspace"), Path::new(r"src\lib.rs"));
    /// ```
    ///
    /// # Windows
    ///
    /// On Windows, both paths are resolved with the supplied cwd using Node's
    /// lexical drive/root rules before their UTF-16 relative comparison. No
    /// per-drive environment variables are read. An ordinary cwd on another
    /// drive selects that drive's root, as in [`SugarPath::absolutize_with`].
    /// Cwd-independent relative inputs may still be compared with an invalid
    /// cwd; this is a sugar_path extension rather than a Node public API.
    ///
    /// # Panics
    ///
    /// Panics if the calculation needs `cwd` and `cwd` is not absolute.
    fn relative_with(
        &self,
        base: impl AsRef<Path>,
        cwd: impl AsRef<Path> + Into<PathBuf>,
    ) -> Cow<'_, Path>;

    /// Converts native separators to `/`, requiring valid UTF-8.
    ///
    /// This operation does not normalize components. It returns a borrowed
    /// string when the path is valid UTF-8 and no separator replacement needs
    /// a new buffer.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use std::path::PathBuf;
    /// use sugar_path::SugarPath;
    ///
    /// let path = PathBuf::from("src").join("lib.rs");
    /// assert_eq!(path.to_slash(), "src/lib.rs");
    /// ```
    ///
    /// # Panics
    ///
    /// Panics if this native path is not valid UTF-8. Use
    /// [`SugarPath::try_to_slash`] to preserve that failure or
    /// [`SugarPath::to_slash_lossy`] to replace invalid encoding.
    fn to_slash(&self) -> Cow<'_, str>;

    /// Converts native separators to `/`, returning `None` for invalid UTF-8.
    ///
    /// This is the non-panicking counterpart of [`SugarPath::to_slash`]. It never
    /// inserts replacement characters: valid UTF-8 yields the slash-separated
    /// string, and invalid native encoding yields `None`.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use std::path::PathBuf;
    /// use sugar_path::SugarPath;
    ///
    /// let path = PathBuf::from("src").join("lib.rs");
    /// assert_eq!(path.try_to_slash().as_deref(), Some("src/lib.rs"));
    /// ```
    fn try_to_slash(&self) -> Option<Cow<'_, str>>;

    /// Converts native separators to `/`, replacing invalid encoding with the
    /// Unicode replacement character.
    ///
    /// Valid UTF-8 follows the same borrowing behavior as
    /// [`SugarPath::to_slash`]. Replacement is irreversible: a result containing
    /// `U+FFFD` may not round-trip to the original native path. Prefer
    /// [`SugarPath::to_slash`] when valid UTF-8 is an invariant, and
    /// [`SugarPath::try_to_slash`] when the original native path must be preserved.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use std::path::PathBuf;
    /// use sugar_path::SugarPath;
    ///
    /// let path = PathBuf::from("src").join("lib.rs");
    /// assert_eq!(path.to_slash_lossy(), "src/lib.rs");
    /// ```
    fn to_slash_lossy(&self) -> Cow<'_, str>;

    /// Views this value as a standard [`Path`] without allocating.
    ///
    /// This is primarily useful for `str` and [`String`] receivers. It performs
    /// no normalization, encoding conversion, or filesystem access.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use std::path::PathBuf;
    /// use sugar_path::SugarPath;
    ///
    /// assert_eq!("src".as_path().join("lib.rs"), PathBuf::from("src").join("lib.rs"));
    /// ```
    fn as_path(&self) -> &Path;
}
impl SugarPath for Path {
    fn normalize(&self) -> Cow<'_, Path> {
        normalize_path(self, TrailingSeparator::Preserve)
    }

    fn absolutize(&self) -> Cow<'_, Path> {
        self.try_absolutize()
            .expect("failed to resolve path against the current directory")
    }

    fn try_absolutize(&self) -> io::Result<Cow<'_, Path>> {
        #[cfg(windows)]
        {
            let result = super::windows_lexical::resolve(&[self])?;
            return Ok(if result.as_os_str() == self.as_os_str() {
                Cow::Borrowed(self)
            } else {
                Cow::Owned(result)
            });
        }
        #[cfg(not(windows))]
        {
            if self.is_absolute() {
                return Ok(normalize_for_resolution(self));
            }

            let cwd = try_get_current_dir()?;
            Ok(self.absolutize_with(cwd))
        }
    }

    fn absolutize_with(&self, cwd: impl AsRef<Path> + Into<PathBuf>) -> Cow<'_, Path> {
        #[cfg(windows)]
        {
            let result = super::windows_lexical::resolve_with(&[cwd.as_ref(), self], |drive| {
                super::windows_cwd::select(
                    drive,
                    |_| None,
                    || {
                        assert!(
                            super::windows_lexical::is_fully_qualified(cwd.as_ref()),
                            "explicit current directory must be absolute"
                        );
                        Ok(cwd.as_ref().to_owned())
                    },
                )
            })
            .expect("explicit cwd resolution is infallible");
            return if result.as_os_str() == self.as_os_str() {
                Cow::Borrowed(self)
            } else {
                Cow::Owned(result)
            };
        }
        #[cfg(not(windows))]
        {
            if self.is_absolute() {
                return normalize_for_resolution(self);
            }

            assert!(
                cwd.as_ref().is_absolute(),
                "explicit current directory must be absolute"
            );

            let mut resolved: PathBuf = cwd.into();
            resolved.push(self);
            Cow::Owned(normalize_owned_for_resolution(resolved))
        }
    }

    fn relative(&self, base: impl AsRef<Path>) -> Cow<'_, Path> {
        self.try_relative(base)
            .expect("failed to resolve relative paths against the current directory")
    }

    fn try_relative(&self, base: impl AsRef<Path>) -> io::Result<Cow<'_, Path>> {
        try_relative_outcome(self, base.as_ref()).map(RelativeOutcome::into_cow_path)
    }

    fn relative_with(
        &self,
        base: impl AsRef<Path>,
        cwd: impl AsRef<Path> + Into<PathBuf>,
    ) -> Cow<'_, Path> {
        relative_outcome_with(self, base.as_ref(), cwd).into_cow_path()
    }

    fn to_slash(&self) -> Cow<'_, str> {
        self.try_to_slash().expect("path is not valid Unicode")
    }

    fn try_to_slash(&self) -> Option<Cow<'_, str>> {
        if std::path::MAIN_SEPARATOR == '/' {
            self.to_str().map(Cow::Borrowed)
        } else {
            self.to_str().map(|s| match replace_main_separator(s) {
                Some(replaced) => Cow::Owned(replaced),
                None => Cow::Borrowed(s),
            })
        }
    }

    fn to_slash_lossy(&self) -> Cow<'_, str> {
        if std::path::MAIN_SEPARATOR == '/' {
            self.to_string_lossy()
        } else {
            match self.to_string_lossy() {
                Cow::Borrowed(s) => match replace_main_separator(s) {
                    Some(replaced) => Cow::Owned(replaced),
                    None => Cow::Borrowed(s),
                },
                Cow::Owned(owned) => match replace_main_separator(&owned) {
                    Some(replaced) => Cow::Owned(replaced),
                    None => Cow::Owned(owned),
                },
            }
        }
    }

    fn as_path(&self) -> &Path {
        self
    }
}

impl SugarPath for str {
    fn normalize(&self) -> Cow<'_, Path> {
        Path::new(self).normalize()
    }

    fn absolutize(&self) -> Cow<'_, Path> {
        Path::new(self).absolutize()
    }

    fn try_absolutize(&self) -> io::Result<Cow<'_, Path>> {
        Path::new(self).try_absolutize()
    }

    fn absolutize_with(&self, cwd: impl AsRef<Path> + Into<PathBuf>) -> Cow<'_, Path> {
        Path::new(self).absolutize_with(cwd)
    }

    fn relative(&self, base: impl AsRef<Path>) -> Cow<'_, Path> {
        Path::new(self).relative(base)
    }

    fn try_relative(&self, base: impl AsRef<Path>) -> io::Result<Cow<'_, Path>> {
        Path::new(self).try_relative(base)
    }

    fn relative_with(
        &self,
        base: impl AsRef<Path>,
        cwd: impl AsRef<Path> + Into<PathBuf>,
    ) -> Cow<'_, Path> {
        Path::new(self).relative_with(base, cwd)
    }

    fn to_slash(&self) -> Cow<'_, str> {
        if std::path::MAIN_SEPARATOR == '/' {
            Cow::Borrowed(self)
        } else {
            match replace_main_separator(self) {
                Some(replaced) => Cow::Owned(replaced),
                None => Cow::Borrowed(self),
            }
        }
    }

    fn try_to_slash(&self) -> Option<Cow<'_, str>> {
        Some(self.to_slash())
    }

    fn to_slash_lossy(&self) -> Cow<'_, str> {
        self.to_slash()
    }

    fn as_path(&self) -> &Path {
        Path::new(self)
    }
}
