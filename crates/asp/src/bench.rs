use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use asp_core::fork::{clone_tree, CloneMethod};
use asp_core::{Error, ErrorCode, Result};
use serde::Serialize;

use crate::ui;

#[derive(Debug, Serialize)]
pub struct BenchSelfReport {
    pub path: PathBuf,
    pub platform: BenchPlatform,
    pub filesystem: BenchFilesystem,
    pub capabilities: BenchCapabilities,
    pub prerequisites: Vec<BenchPrerequisite>,
    pub recommendations: Vec<String>,
    pub probe_errors: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct BenchPlatform {
    pub os: &'static str,
    pub arch: &'static str,
    pub supported: bool,
    pub support_hint: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct BenchFilesystem {
    pub kind: Option<String>,
    pub case_sensitive: bool,
    pub symlinks: bool,
    pub hardlinks: bool,
    pub atomic_rename: bool,
}

#[derive(Debug, Serialize)]
pub struct BenchCapabilities {
    pub directory_clone_method: Option<CloneMethod>,
    pub copy_on_write_forks: bool,
    pub large_file_sidecar_cow: bool,
    pub same_volume_forks_required: bool,
}

#[derive(Debug, Serialize)]
pub struct BenchPrerequisite {
    pub id: &'static str,
    pub name: &'static str,
    pub ok: bool,
    pub severity: &'static str,
    pub summary: String,
    pub hint: Option<String>,
}

pub fn self_report(path: &Path) -> Result<BenchSelfReport> {
    let path = path.canonicalize().map_err(|e| {
        Error::new(
            ErrorCode::Io,
            format!("cannot inspect {}: {e}", path.display()),
        )
        .with_hint("choose an existing directory with `asp -C <dir> bench self`")
        .with_source(e)
    })?;
    if !path.is_dir() {
        return Err(Error::new(
            ErrorCode::Io,
            format!("bench self target is not a directory: {}", path.display()),
        )
        .with_hint("choose an existing directory with `asp -C <dir> bench self`"));
    }

    let support_hint = asp_core::ensure_supported_platform()
        .err()
        .and_then(|err| err.hint);
    let supported = support_hint.is_none();
    let probe_dir = ProbeDir::new(&path)?;
    let mut probe_errors = Vec::new();

    let directory_clone_method = if supported {
        match probe_directory_clone(probe_dir.path()) {
            Ok(method) => Some(method),
            Err(err) => {
                probe_errors.push(format!("directory clone probe failed: {}", err.message));
                None
            }
        }
    } else {
        None
    };

    let filesystem = BenchFilesystem {
        kind: filesystem_kind(&path),
        case_sensitive: record_probe(&mut probe_errors, "case sensitivity", || {
            probe_case_sensitive(probe_dir.path())
        }),
        symlinks: record_probe(&mut probe_errors, "symlink", || {
            probe_symlink(probe_dir.path())
        }),
        hardlinks: record_probe(&mut probe_errors, "hardlink", || {
            probe_hardlink(probe_dir.path())
        }),
        atomic_rename: record_probe(&mut probe_errors, "atomic rename", || {
            probe_atomic_rename(probe_dir.path())
        }),
    };

    let copy_on_write_forks = matches!(
        directory_clone_method,
        Some(CloneMethod::Clonefile | CloneMethod::Reflink)
    );
    let capabilities = BenchCapabilities {
        directory_clone_method,
        copy_on_write_forks,
        large_file_sidecar_cow: copy_on_write_forks,
        same_volume_forks_required: true,
    };
    let prerequisites = prerequisites(
        supported,
        support_hint.as_deref(),
        &filesystem,
        &capabilities,
    );
    let recommendations = recommendations(supported, &filesystem, &capabilities);

    Ok(BenchSelfReport {
        path,
        platform: BenchPlatform {
            os: std::env::consts::OS,
            arch: std::env::consts::ARCH,
            supported,
            support_hint,
        },
        filesystem,
        capabilities,
        prerequisites,
        recommendations,
        probe_errors,
    })
}

pub fn print_self_report(report: &BenchSelfReport) {
    println!("{}", ui::bold("bench self"));
    println!("  path:       {}", report.path.display());
    println!(
        "  platform:   {} {} {}",
        report.platform.os,
        report.platform.arch,
        if report.platform.supported {
            ui::green("(supported)")
        } else {
            ui::yellow("(unsupported)")
        }
    );
    if let Some(hint) = &report.platform.support_hint {
        println!("  hint:       {hint}");
    }
    println!(
        "  filesystem: {}",
        report.filesystem.kind.as_deref().unwrap_or("unknown")
    );
    println!(
        "  dir clone:  {}",
        match report.capabilities.directory_clone_method {
            Some(CloneMethod::Clonefile) => ui::green("clonefile (copy-on-write)"),
            Some(CloneMethod::Reflink) => ui::green("reflink (copy-on-write)"),
            Some(CloneMethod::Copy) => ui::yellow("copy (no CoW detected)"),
            None => ui::yellow("unavailable"),
        }
    );
    println!("  case sens.: {}", yes_no(report.filesystem.case_sensitive));
    println!("  symlinks:   {}", yes_no(report.filesystem.symlinks));
    println!("  hardlinks:  {}", yes_no(report.filesystem.hardlinks));
    println!("  rename:     {}", yes_no(report.filesystem.atomic_rename));

    if !report.prerequisites.is_empty() {
        println!();
        println!("{}", ui::bold("prerequisites"));
        for prerequisite in &report.prerequisites {
            let status = if prerequisite.ok {
                ui::green("ok")
            } else if prerequisite.severity == "error" {
                ui::red("error")
            } else {
                ui::yellow("warning")
            };
            println!("  {status} {}: {}", prerequisite.name, prerequisite.summary);
            if !prerequisite.ok {
                if let Some(hint) = &prerequisite.hint {
                    println!("    hint: {hint}");
                }
            }
        }
    }

    if !report.recommendations.is_empty() {
        println!();
        println!("{}", ui::bold("recommendations"));
        for recommendation in &report.recommendations {
            println!("  - {recommendation}");
        }
    }
    if !report.probe_errors.is_empty() {
        println!();
        println!("{}", ui::yellow("probe warnings"));
        for error in &report.probe_errors {
            println!("  - {error}");
        }
    }
}

fn prerequisites(
    supported: bool,
    support_hint: Option<&str>,
    filesystem: &BenchFilesystem,
    capabilities: &BenchCapabilities,
) -> Vec<BenchPrerequisite> {
    vec![
        BenchPrerequisite {
            id: "platform.supported",
            name: "platform support",
            ok: supported,
            severity: if supported { "info" } else { "error" },
            summary: if supported {
                "workspace commands are enabled on this platform".to_string()
            } else {
                "workspace commands are disabled on this platform".to_string()
            },
            hint: support_hint.map(str::to_string),
        },
        BenchPrerequisite {
            id: "filesystem.symlinks",
            name: "symlink privilege",
            ok: filesystem.symlinks,
            severity: if filesystem.symlinks {
                "info"
            } else {
                "warning"
            },
            summary: symlink_prerequisite_summary(filesystem.symlinks),
            hint: (!filesystem.symlinks).then(|| symlink_prerequisite_hint().to_string()),
        },
        BenchPrerequisite {
            id: "filesystem.hardlinks",
            name: "hardlink support",
            ok: filesystem.hardlinks,
            severity: if filesystem.hardlinks {
                "info"
            } else {
                "warning"
            },
            summary: if filesystem.hardlinks {
                "hardlink probe succeeded on this path".to_string()
            } else {
                "hardlink probe failed on this path".to_string()
            },
            hint: (!filesystem.hardlinks).then(|| {
                "choose a local filesystem that permits hardlinks, or ask IT to allow them for the workspace path".to_string()
            }),
        },
        BenchPrerequisite {
            id: "filesystem.atomic_rename",
            name: "atomic rename",
            ok: filesystem.atomic_rename,
            severity: if filesystem.atomic_rename {
                "info"
            } else {
                "error"
            },
            summary: if filesystem.atomic_rename {
                "write-temp-plus-rename probe succeeded on this path".to_string()
            } else {
                "atomic rename probe failed on this path".to_string()
            },
            hint: (!filesystem.atomic_rename).then(|| {
                "move the workspace to a local filesystem with atomic rename semantics, then rerun `asp bench self --json`".to_string()
            }),
        },
        BenchPrerequisite {
            id: "fork.copy_on_write",
            name: "copy-on-write forks",
            ok: capabilities.copy_on_write_forks,
            severity: if capabilities.copy_on_write_forks {
                "info"
            } else {
                "warning"
            },
            summary: if capabilities.copy_on_write_forks {
                "directory clone probe found a copy-on-write fork method".to_string()
            } else {
                "no copy-on-write fork method was found for this path".to_string()
            },
            hint: (!capabilities.copy_on_write_forks).then(|| {
                "use APFS on macOS or btrfs/XFS with reflink on Linux for fastest forks; correctness does not require CoW".to_string()
            }),
        },
    ]
}

fn symlink_prerequisite_summary(ok: bool) -> String {
    if cfg!(windows) {
        if ok {
            "file symlink probe succeeded; Developer Mode or SeCreateSymbolicLinkPrivilege is available for this path".to_string()
        } else {
            "file symlink probe failed; Developer Mode or SeCreateSymbolicLinkPrivilege is missing or blocked for this path".to_string()
        }
    } else if ok {
        "symlink creation and readlink probe succeeded on this path".to_string()
    } else {
        "symlink creation or readlink probe failed on this path".to_string()
    }
}

fn symlink_prerequisite_hint() -> &'static str {
    if cfg!(windows) {
        "enable Windows Developer Mode or grant the Create symbolic links (SeCreateSymbolicLinkPrivilege) right, then rerun `asp bench self --json`"
    } else {
        "choose a filesystem and security policy that allow symlinks, then rerun `asp bench self --json`"
    }
}

fn yes_no(value: bool) -> String {
    if value {
        ui::green("yes")
    } else {
        ui::yellow("no")
    }
}

fn recommendations(
    supported: bool,
    filesystem: &BenchFilesystem,
    capabilities: &BenchCapabilities,
) -> Vec<String> {
    let mut out = Vec::new();
    if !supported {
        out.push("use WSL2, macOS, or Linux for asp workspace operations".to_string());
    }
    if !capabilities.copy_on_write_forks {
        out.push(
            "use APFS on macOS or btrfs/XFS with reflink on Linux for fastest forks".to_string(),
        );
    }
    if !filesystem.case_sensitive {
        out.push("this path is case-insensitive; asp guards case-only restores, but avoid relying on paths that differ only by case".to_string());
    }
    if !filesystem.symlinks {
        out.push(symlink_prerequisite_hint().to_string());
    }
    if !filesystem.atomic_rename {
        out.push(
            "atomic rename probe failed; avoid placing asp workspaces on this filesystem"
                .to_string(),
        );
    }
    if out.is_empty() {
        out.push(
            "this path has the expected local filesystem capabilities for asp benchmarks"
                .to_string(),
        );
    }
    out
}

fn record_probe<F>(errors: &mut Vec<String>, name: &str, f: F) -> bool
where
    F: FnOnce() -> Result<bool>,
{
    match f() {
        Ok(value) => value,
        Err(err) => {
            errors.push(format!("{name} probe failed: {}", err.message));
            false
        }
    }
}

fn probe_directory_clone(dir: &Path) -> Result<CloneMethod> {
    let src = dir.join("clone-src");
    let dst = dir.join("clone-dst");
    fs::create_dir(&src)?;
    fs::create_dir(src.join("sub"))?;
    fs::write(src.join("sub/file.txt"), "asp bench self\n")?;
    clone_tree(&src, &dst)
}

fn probe_case_sensitive(dir: &Path) -> Result<bool> {
    let lower = dir.join("case_probe");
    let upper = dir.join("CASE_PROBE");
    fs::write(&lower, "lower")?;
    match OpenOptions::new().write(true).create_new(true).open(&upper) {
        Ok(_) => Ok(true),
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
        Err(err) => Err(err.into()),
    }
}

fn probe_hardlink(dir: &Path) -> Result<bool> {
    let src = dir.join("hardlink-source");
    let dst = dir.join("hardlink-dst");
    fs::write(&src, "hardlink")?;
    Ok(fs::hard_link(&src, &dst).is_ok())
}

fn probe_atomic_rename(dir: &Path) -> Result<bool> {
    let src = dir.join("rename-source");
    let dst = dir.join("rename-dst");
    fs::write(&src, "rename")?;
    Ok(fs::rename(&src, &dst).is_ok() && dst.exists() && !src.exists())
}

#[cfg(unix)]
fn probe_symlink(dir: &Path) -> Result<bool> {
    let src = dir.join("symlink-source");
    let dst = dir.join("symlink-dst");
    fs::write(&src, "symlink")?;
    Ok(std::os::unix::fs::symlink("symlink-source", &dst).is_ok() && fs::read_link(&dst).is_ok())
}

#[cfg(windows)]
fn probe_symlink(dir: &Path) -> Result<bool> {
    let src = dir.join("symlink-source");
    let dst = dir.join("symlink-dst");
    fs::write(&src, "symlink")?;
    Ok(std::os::windows::fs::symlink_file(&src, &dst).is_ok() && fs::read_link(&dst).is_ok())
}

#[cfg(not(any(unix, windows)))]
fn probe_symlink(_dir: &Path) -> Result<bool> {
    Ok(false)
}

#[cfg(target_os = "macos")]
fn filesystem_kind(path: &Path) -> Option<String> {
    use std::ffi::{CStr, CString};
    use std::mem::MaybeUninit;
    use std::os::unix::ffi::OsStrExt;

    let c_path = CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut stat = MaybeUninit::<libc::statfs>::uninit();
    let rc = unsafe { libc::statfs(c_path.as_ptr(), stat.as_mut_ptr()) };
    if rc != 0 {
        return None;
    }
    let stat = unsafe { stat.assume_init() };
    let c_name = unsafe { CStr::from_ptr(stat.f_fstypename.as_ptr()) };
    Some(c_name.to_string_lossy().into_owned())
}

#[cfg(target_os = "linux")]
fn filesystem_kind(path: &Path) -> Option<String> {
    use std::ffi::CString;
    use std::mem::MaybeUninit;
    use std::os::unix::ffi::OsStrExt;

    let c_path = CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut stat = MaybeUninit::<libc::statfs>::uninit();
    let rc = unsafe { libc::statfs(c_path.as_ptr(), stat.as_mut_ptr()) };
    if rc != 0 {
        return None;
    }
    let stat = unsafe { stat.assume_init() };
    Some(match stat.f_type {
        0x9123_683e => "btrfs".to_string(),
        0xef53 => "ext2/ext3/ext4".to_string(),
        0x5846_5342 => "xfs".to_string(),
        0x0102_1994 => "tmpfs".to_string(),
        0x6969 => "nfs".to_string(),
        other => format!("0x{other:x}"),
    })
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn filesystem_kind(_path: &Path) -> Option<String> {
    None
}

struct ProbeDir {
    path: PathBuf,
}

impl ProbeDir {
    fn new(parent: &Path) -> Result<Self> {
        let pid = std::process::id();
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        for attempt in 0..100 {
            let path = parent.join(format!(".asp-bench-self-{pid}-{stamp}-{attempt}"));
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self { path }),
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(err) => {
                    return Err(Error::new(
                        ErrorCode::Io,
                        format!("cannot create bench self probe directory: {err}"),
                    )
                    .with_hint("run `asp bench self` in a writable directory")
                    .with_source(err));
                }
            }
        }
        Err(Error::new(
            ErrorCode::Io,
            "cannot allocate a unique bench self probe directory",
        )
        .with_hint("remove stale .asp-bench-self-* directories and retry"))
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for ProbeDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filesystem(symlinks: bool, hardlinks: bool, atomic_rename: bool) -> BenchFilesystem {
        BenchFilesystem {
            kind: None,
            case_sensitive: true,
            symlinks,
            hardlinks,
            atomic_rename,
        }
    }

    fn capabilities(copy_on_write_forks: bool) -> BenchCapabilities {
        BenchCapabilities {
            directory_clone_method: None,
            copy_on_write_forks,
            large_file_sidecar_cow: copy_on_write_forks,
            same_volume_forks_required: true,
        }
    }

    #[test]
    fn prerequisites_are_machine_readable_and_actionable() {
        let checks = prerequisites(
            false,
            Some("use WSL2 or run `asp bench self --json`"),
            &filesystem(false, true, false),
            &capabilities(false),
        );

        let platform = checks
            .iter()
            .find(|check| check.id == "platform.supported")
            .unwrap();
        assert!(!platform.ok);
        assert_eq!(platform.severity, "error");
        assert!(platform.hint.as_deref().unwrap().contains("WSL2"));

        let symlinks = checks
            .iter()
            .find(|check| check.id == "filesystem.symlinks")
            .unwrap();
        assert!(!symlinks.ok);
        assert!(symlinks.hint.as_deref().unwrap().contains("asp bench self"));

        let atomic = checks
            .iter()
            .find(|check| check.id == "filesystem.atomic_rename")
            .unwrap();
        assert!(!atomic.ok);
        assert_eq!(atomic.severity, "error");
    }
}
