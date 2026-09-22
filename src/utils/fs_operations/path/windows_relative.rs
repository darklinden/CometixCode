//! Maps to Node v24.14.0 lib/path.js win32.relative, after resolving both inputs.
//! UTF-16 offsets preserve JS slicing, including case mappings that change length.
use std::path::{Path, PathBuf};

pub(super) fn wide(path: &Path) -> Vec<u16> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        path.as_os_str().encode_wide().collect()
    }
    #[cfg(not(windows))]
    {
        path.to_str()
            .expect("portable oracle paths are Unicode")
            .encode_utf16()
            .collect()
    }
}
fn native(value: &[u16]) -> PathBuf {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStringExt;
        std::ffi::OsString::from_wide(value).into()
    }
    #[cfg(not(windows))]
    {
        PathBuf::from(String::from_utf16(value).expect("portable oracle paths are Unicode"))
    }
}
fn lower(value: &[u16]) -> Vec<u16> {
    let mut result = Vec::new();
    let mut run = String::new();
    for c in char::decode_utf16(value.iter().copied()) {
        match c {
            Ok(c) => run.push(c),
            Err(e) => {
                result.extend(run.to_lowercase().encode_utf16());
                run.clear();
                result.push(e.unpaired_surrogate());
            }
        }
    }
    result.extend(run.to_lowercase().encode_utf16());
    result
}
fn joined(parts: &[&[u16]]) -> Vec<u16> {
    parts.join(&92)
}

pub(crate) fn from_resolved(base: &Path, target: &Path) -> PathBuf {
    let a = wide(base);
    let b = wide(target);
    if a == b {
        return PathBuf::new();
    }
    let from = lower(&a);
    let to = lower(&b);
    if from == to {
        return PathBuf::new();
    }
    if a.len() != from.len() || b.len() != to.len() {
        let mut f: Vec<_> = a.split(|c| *c == 92).collect();
        let mut t: Vec<_> = b.split(|c| *c == 92).collect();
        if f.last() == Some(&&[][..]) {
            f.pop();
        }
        if t.last() == Some(&&[][..]) {
            t.pop();
        }
        let common = f
            .iter()
            .zip(&t)
            .take_while(|(a, b)| lower(a) == lower(b))
            .count();
        if common == 0 {
            return target.to_owned();
        }
        let parents = [46u16, 46u16];
        let mut parts = vec![&parents[..]; f.len() - common];
        parts.extend_from_slice(&t[common..]);
        return native(&joined(&parts));
    }
    let start = |p: &[u16]| p.iter().take_while(|c| **c == 92).count();
    let fs = start(&from);
    let ts = start(&to);
    let mut fe = from.len();
    while fe.saturating_sub(1) > fs && from[fe - 1] == 92 {
        fe -= 1;
    }
    let mut te = to.len();
    while te.saturating_sub(1) > ts && to[te - 1] == 92 {
        te -= 1;
    }
    let fl = fe - fs;
    let tl = te - ts;
    let len = fl.min(tl);
    let mut common = None;
    let mut i = 0;
    while i < len {
        if from[fs + i] != to[ts + i] {
            break;
        }
        if from[fs + i] == 92 {
            common = Some(i);
        }
        i += 1;
    }
    if i != len {
        if common.is_none() {
            return target.to_owned();
        }
    } else {
        if tl > len {
            if to[ts + i] == 92 {
                return native(&b[ts + i + 1..]);
            }
            if i == 2 {
                return native(&b[ts + i..]);
            }
        }
        if fl > len {
            if from[fs + i] == 92 {
                common = Some(i);
            } else if i == 2 {
                common = Some(3);
            }
        }
        if common.is_none() {
            common = Some(0);
        }
    }
    let common = common.unwrap();
    let mut out = Vec::new();
    for i in fs + common + 1..=fe {
        if i == fe || from[i] == 92 {
            if !out.is_empty() {
                out.push(92);
            }
            out.extend_from_slice(&[46, 46]);
        }
    }
    let mut ts = ts + common;
    if !out.is_empty() {
        out.extend_from_slice(&b[ts..te]);
        return native(&out);
    }
    if b.get(ts) == Some(&92) {
        ts += 1;
    }
    native(&b[ts..te])
}
