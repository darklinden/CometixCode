//! Windows dependency carrier for Node symlinkSync(..., 'junction').
//! Mount-point reparse buffers follow Windows REPARSE_DATA_BUFFER layout.
use std::{
    fs, io,
    os::windows::{
        ffi::{OsStrExt, OsStringExt},
        fs::OpenOptionsExt,
        io::AsRawHandle,
    },
    path::Path,
};

/// Maps to Node internal/fs/utils.js#preprocessSymlinkDestination for non-junctions.
pub(super) fn symlink_destination(target: &Path) -> io::Result<std::path::PathBuf> {
    if super::native::is_absolute(target) {
        to_namespaced_path(target)
    } else {
        let value = target
            .as_os_str()
            .encode_wide()
            .map(|unit| {
                if unit == b'/' as u16 {
                    b'\\' as u16
                } else {
                    unit
                }
            })
            .collect::<Vec<_>>();
        Ok(std::ffi::OsString::from_wide(&value).into())
    }
}

/// Maps to Node win32.toNamespacedPath. Existing device namespaces retain their
/// resolved spelling, as do drive and UNC paths.
fn to_namespaced_path(path: &Path) -> io::Result<std::path::PathBuf> {
    let resolved = super::path::windows_lexical::resolve(&[path])?;
    let value = resolved.as_os_str().encode_wide().collect::<Vec<_>>();
    if value.len() <= 2 {
        return Ok(path.to_owned());
    }
    let slash = b'\\' as u16;
    let result = if value.len() > 2
        && value[0] == slash
        && value[1] == slash
        && ![b'?' as u16, b'.' as u16].contains(&value[2])
    {
        r"\\?\UNC\"
            .encode_utf16()
            .chain(value[2..].iter().copied())
            .collect::<Vec<_>>()
    } else if value.len() > 2
        && ((b'A' as u16..=b'Z' as u16).contains(&value[0])
            || (b'a' as u16..=b'z' as u16).contains(&value[0]))
        && value[1] == b':' as u16
        && value[2] == slash
    {
        r"\\?\".encode_utf16().chain(value).collect::<Vec<_>>()
    } else {
        return Ok(resolved);
    };
    Ok(std::ffi::OsString::from_wide(&result).into())
}

/// Maps to libuv win/fs.c#fs__create_junction: strip the long-path prefix,
/// collapse slash runs, and retain native UTF-16 in the reparse buffer.
pub(super) fn junction_names(target: &Path) -> io::Result<(Vec<u16>, Vec<u16>)> {
    let value = target.as_os_str().encode_wide().collect::<Vec<_>>();
    let namespace = r"\\?\".encode_utf16().collect::<Vec<_>>();
    let value = if let Some(rest) = value.strip_prefix(namespace.as_slice()) {
        rest
    } else if value.len() >= 3
        && ((b'A' as u16..=b'Z' as u16).contains(&value[0])
            || (b'a' as u16..=b'z' as u16).contains(&value[0]))
        && value[1] == b':' as u16
        && [b'/' as u16, b'\\' as u16].contains(&value[2])
    {
        &value[..]
    } else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            super::FsError {
                code: "EINVAL",
                syscall: Some("symlink"),
                path: Some(target.to_owned()),
                dest: None,
                message: "EINVAL: invalid junction target".into(),
                native: None,
            },
        ));
    };
    let mut display = Vec::new();
    let mut pending_slash = false;
    for &unit in value {
        if [b'/' as u16, b'\\' as u16].contains(&unit) {
            pending_slash = true;
        } else {
            if pending_slash {
                display.push(b'\\' as u16);
                pending_slash = false;
            }
            display.push(unit);
        }
    }
    let mut substitute = r"\??\"
        .encode_utf16()
        .chain(display.iter().copied())
        .collect::<Vec<_>>();
    if pending_slash {
        substitute.push(b'\\' as u16);
    }
    if pending_slash || display.len() == 2 {
        display.push(b'\\' as u16);
    }
    Ok((substitute, display))
}

pub(super) fn junction(target: &Path, path: &Path) -> io::Result<()> {
    // Maps to Node 24.14 internal/fs/utils.js#preprocessSymlinkDestination:
    // a relative junction target belongs to the link's parent, not process cwd.
    let absolute = super::path::windows_lexical::resolve(&[path, Path::new(".."), target])?;
    let namespaced = to_namespaced_path(&absolute)?;
    let (substitute, display) = junction_names(&namespaced)?;
    let data_size = 8 + (substitute.len() + 1 + display.len() + 1) * 2;
    if data_size + 8 > 16 * 1024 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "junction target too long",
        ));
    }
    let mut data = Vec::with_capacity(data_size + 8);
    data.extend_from_slice(&0xa0000003_u32.to_le_bytes()); // IO_REPARSE_TAG_MOUNT_POINT
    data.extend_from_slice(&(data_size as u16).to_le_bytes());
    data.extend_from_slice(&0_u16.to_le_bytes());
    for field in [
        0,
        (substitute.len() * 2) as u16,
        ((substitute.len() + 1) * 2) as u16,
        (display.len() * 2) as u16,
    ] {
        data.extend_from_slice(&field.to_le_bytes());
    }
    for unit in substitute.into_iter().chain([0]).chain(display).chain([0]) {
        data.extend_from_slice(&unit.to_le_bytes());
    }
    fs::create_dir(path)?;
    let result = (|| {
        let file = fs::OpenOptions::new()
            .write(true)
            .custom_flags(0x02200000)
            .open(path)?; // OPEN_REPARSE_POINT | BACKUP_SEMANTICS
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn DeviceIoControl(
                handle: *mut std::ffi::c_void,
                code: u32,
                input: *const u8,
                input_size: u32,
                output: *mut u8,
                output_size: u32,
                returned: *mut u32,
                overlapped: *mut std::ffi::c_void,
            ) -> i32;
        }
        let mut returned = 0;
        // SAFETY: valid owned handle; correctly sized initialized input buffer;
        // synchronous call does not retain the pointers.
        let success = unsafe {
            DeviceIoControl(
                file.as_raw_handle(),
                0x000900a4,
                data.as_ptr(),
                data.len() as u32,
                std::ptr::null_mut(),
                0,
                &mut returned,
                std::ptr::null_mut(),
            )
        };
        if success == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    })();
    if result.is_err() {
        let _ = fs::remove_dir(path);
    }
    result
}

/// Windows stat carrier: stable Rust Metadata omits volume/file identity,
/// link count and change time, so query them from the same opened handle.
pub(super) fn stat(path: &Path, follow: bool) -> io::Result<super::FsStats> {
    #[repr(C)]
    #[derive(Default)]
    struct FileTime {
        low: u32,
        high: u32,
    }
    #[repr(C)]
    #[derive(Default)]
    struct FileInformation {
        attributes: u32,
        creation: FileTime,
        access: FileTime,
        write: FileTime,
        volume: u32,
        size_high: u32,
        size_low: u32,
        links: u32,
        index_high: u32,
        index_low: u32,
    }
    #[repr(C)]
    #[derive(Default)]
    struct BasicInformation {
        creation: i64,
        access: i64,
        write: i64,
        change: i64,
        attributes: u32,
    }
    #[repr(C)]
    #[derive(Default)]
    struct StandardInformation {
        allocation: i64,
        end: i64,
        links: u32,
        delete_pending: u8,
        directory: u8,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetFileInformationByHandle(
            handle: *mut std::ffi::c_void,
            info: *mut FileInformation,
        ) -> i32;
        fn GetFileInformationByHandleEx(
            handle: *mut std::ffi::c_void,
            class: u32,
            info: *mut std::ffi::c_void,
            size: u32,
        ) -> i32;
    }
    let flags = 0x02000000 | if follow { 0 } else { 0x00200000 };
    let file = fs::OpenOptions::new()
        .access_mode(0)
        .share_mode(7)
        .custom_flags(flags)
        .open(path)?;
    let result = (|| {
        let mut result: super::FsStats = file.metadata()?.into();
        let mut identity = FileInformation::default();
        let mut basic = BasicInformation::default();
        let mut standard = StandardInformation::default();
        // SAFETY: all buffers use the Windows ABI layout, have the advertised
        // size and remain valid until each synchronous call returns.
        let success = unsafe {
            GetFileInformationByHandle(file.as_raw_handle(), &mut identity) != 0
                && GetFileInformationByHandleEx(
                    file.as_raw_handle(),
                    0,
                    (&mut basic as *mut BasicInformation).cast(),
                    std::mem::size_of::<BasicInformation>() as u32,
                ) != 0
                && GetFileInformationByHandleEx(
                    file.as_raw_handle(),
                    1,
                    (&mut standard as *mut StandardInformation).cast(),
                    std::mem::size_of::<StandardInformation>() as u32,
                ) != 0
        };
        if !success {
            return Err(io::Error::last_os_error());
        }
        result.dev = identity.volume as u64;
        result.ino = ((identity.index_high as u64) << 32) | identity.index_low as u64;
        result.nlink = identity.links as u64;
        result.blocks = standard.allocation.max(0) as u64 / 512;
        let milliseconds = |value: i64| (value as f64 - 116444736000000000.) / 10000.;
        result.ctime_ms = milliseconds(basic.change);
        result.atime_ms = milliseconds(basic.access);
        result.mtime_ms = milliseconds(basic.write);
        result.birthtime_ms = milliseconds(basic.creation);
        if result.kind.is_symlink() {
            result.mode = (result.mode & 0o7777) | 0o120000;
        }
        Ok(result)
    })();
    super::handle::finish(file, result)
}
