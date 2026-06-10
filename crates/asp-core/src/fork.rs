//! Whole-directory copy-on-write cloning: `clonefile(2)` on macOS (kernel-
//! recursive, ~1s for 100k files), per-file reflink on Linux, plain copy as
//! the last resort. The fork carries the WHOLE physical tree — untracked
//! files, node_modules, build artifacts — that is what makes it instantly
//! runnable.

use std::path::Path;

use serde::Serialize;

use crate::error::{Error, ErrorCode, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CloneMethod {
    /// macOS APFS kernel-recursive clone — O(inodes), zero data copied.
    Clonefile,
    /// Linux per-file FICLONE reflink (btrfs/XFS) — zero data copied.
    Reflink,
    /// Byte copy fallback (non-CoW filesystem) — works everywhere, slower.
    Copy,
}

/// Clone `src` directory to `dst` (which must not exist) using the fastest
/// available method for the platform/filesystem.
pub fn clone_tree(src: &Path, dst: &Path) -> Result<CloneMethod> {
    if dst.exists() {
        return Err(Error::new(
            ErrorCode::ForkExists,
            format!("destination already exists: {}", dst.display()),
        )
        .with_hint("pick a different fork name, or `asp discard` the old fork first"));
    }
    platform_clone(src, dst)
}

#[cfg(target_os = "macos")]
fn platform_clone(src: &Path, dst: &Path) -> Result<CloneMethod> {
    use std::os::unix::ffi::OsStrExt;
    let c_src = std::ffi::CString::new(src.as_os_str().as_bytes())
        .map_err(|_| Error::new(ErrorCode::Io, "path contains NUL byte"))?;
    let c_dst = std::ffi::CString::new(dst.as_os_str().as_bytes())
        .map_err(|_| Error::new(ErrorCode::Io, "path contains NUL byte"))?;
    // SAFETY: both pointers are valid NUL-terminated C strings for the call.
    let rc = unsafe { libc::clonefile(c_src.as_ptr(), c_dst.as_ptr(), 0) };
    if rc == 0 {
        return Ok(CloneMethod::Clonefile);
    }
    let errno = std::io::Error::last_os_error();
    match errno.raw_os_error() {
        Some(libc::EXDEV) => Err(Error::new(
            ErrorCode::CrossVolume,
            format!(
                "cannot clone across volumes: {} → {}",
                src.display(),
                dst.display()
            ),
        )
        .with_hint(
            "forks must live on the same volume as the workspace (copy-on-write requirement)",
        )),
        // Non-APFS volume (e.g. FAT USB drive): degrade to a real copy.
        Some(libc::ENOTSUP) => copy_recursive(src, dst).map(|_| CloneMethod::Copy),
        _ => {
            Err(Error::new(ErrorCode::Io, format!("clonefile failed: {errno}")).with_source(errno))
        }
    }
}

#[cfg(target_os = "linux")]
fn platform_clone(src: &Path, dst: &Path) -> Result<CloneMethod> {
    // Walk the tree, reflinking each regular file. Falls back to byte copy
    // per-file if the filesystem rejects FICLONE (ext4 etc.).
    let mut any_copied = false;
    reflink_walk(src, dst, &mut any_copied)?;
    Ok(if any_copied {
        CloneMethod::Copy
    } else {
        CloneMethod::Reflink
    })
}

#[cfg(target_os = "linux")]
fn reflink_walk(src: &Path, dst: &Path, any_copied: &mut bool) -> Result<()> {
    use std::os::fd::AsRawFd;
    std::fs::create_dir(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let s = entry.path();
        let d = dst.join(entry.file_name());
        let ft = entry.file_type()?;
        if ft.is_dir() {
            reflink_walk(&s, &d, any_copied)?;
        } else if ft.is_symlink() {
            let target = std::fs::read_link(&s)?;
            std::os::unix::fs::symlink(target, &d)?;
        } else if ft.is_file() {
            let fs_in = std::fs::File::open(&s)?;
            let fs_out = std::fs::File::create(&d)?;
            // SAFETY: valid fds; FICLONE is a no-op on failure.
            let rc = unsafe { libc::ioctl(fs_out.as_raw_fd(), libc::FICLONE, fs_in.as_raw_fd()) };
            if rc != 0 {
                drop(fs_out);
                std::fs::copy(&s, &d)?;
                *any_copied = true;
            } else if let Ok(md) = fs_in.metadata() {
                let _ = fs_out.set_permissions(md.permissions());
            }
        }
        // Sockets/FIFOs are skipped: they are runtime state, not files.
    }
    // Directory permissions are applied AFTER populating children — a
    // read-only source dir must not block writes into its clone mid-walk.
    if let Ok(md) = std::fs::metadata(src) {
        let _ = std::fs::set_permissions(dst, md.permissions());
    }
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn platform_clone(src: &Path, dst: &Path) -> Result<CloneMethod> {
    copy_recursive(src, dst).map(|_| CloneMethod::Copy)
}

#[allow(dead_code)]
fn copy_recursive(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let s = entry.path();
        let d = dst.join(entry.file_name());
        let ft = entry.file_type()?;
        if ft.is_dir() {
            copy_recursive(&s, &d)?;
        } else if ft.is_symlink() {
            let target = std::fs::read_link(&s)?;
            #[cfg(unix)]
            std::os::unix::fs::symlink(target, &d)?;
            #[cfg(not(unix))]
            let _ = target;
        } else if ft.is_file() {
            std::fs::copy(&s, &d)?;
        }
    }
    if let Ok(md) = std::fs::metadata(src) {
        let _ = std::fs::set_permissions(dst, md.permissions());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clone_preserves_content_and_symlinks() {
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("src");
        std::fs::create_dir_all(src.join("sub")).unwrap();
        std::fs::write(src.join("a.txt"), "hello").unwrap();
        std::fs::write(src.join("sub/b.txt"), "world").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("a.txt", src.join("link")).unwrap();

        let dst = d.path().join("dst");
        let method = clone_tree(&src, &dst).unwrap();
        // macOS tempdirs are APFS → kernel clone. Linux /tmp is often tmpfs,
        // where per-file reflink degrades to copy — both are correct there.
        #[cfg(target_os = "macos")]
        assert_eq!(method, CloneMethod::Clonefile);
        #[cfg(not(target_os = "macos"))]
        let _ = method;
        assert_eq!(std::fs::read_to_string(dst.join("a.txt")).unwrap(), "hello");
        assert_eq!(
            std::fs::read_to_string(dst.join("sub/b.txt")).unwrap(),
            "world"
        );
        #[cfg(unix)]
        assert!(dst
            .join("link")
            .symlink_metadata()
            .unwrap()
            .file_type()
            .is_symlink());

        // CoW independence: writing in the clone must not affect the source.
        std::fs::write(dst.join("a.txt"), "changed").unwrap();
        assert_eq!(std::fs::read_to_string(src.join("a.txt")).unwrap(), "hello");
    }

    #[test]
    fn refuses_existing_destination() {
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("src");
        let dst = d.path().join("dst");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(&dst).unwrap();
        let err = clone_tree(&src, &dst).unwrap_err();
        assert_eq!(err.code, ErrorCode::ForkExists);
        assert!(err.hint.is_some());
    }
}
