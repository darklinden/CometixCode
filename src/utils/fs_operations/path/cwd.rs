use std::{borrow::Cow, io, path::Path};

// The product always observes the live cwd; cached_current_dir was never enabled.
pub(super) fn try_get_current_dir() -> io::Result<Cow<'static, Path>> {
    std::env::current_dir().map(Cow::Owned)
}
