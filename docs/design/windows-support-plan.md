# Native Windows Support Plan

Native Windows support means more than making the binary compile. It means every
workspace operation keeps the same trust model users get on macOS and Linux:
recoverable checkpoints, reviewable git state, no writes to the user's `.git`
except `promote`, and actionable JSON errors.

This plan is the gate for replacing today's intentional `unsupported_platform`
behavior with native Windows behavior.

## Sources

- Windows symbolic links:
  <https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-createsymboliclinkw>
- Windows access control overview:
  <https://learn.microsoft.com/en-us/windows/security/identity-protection/access-control/access-control>
- File security and access rights:
  <https://learn.microsoft.com/en-us/windows/win32/fileio/file-security-and-access-rights>
- Windows path and namespace rules:
  <https://learn.microsoft.com/en-us/windows/win32/fileio/naming-a-file>
- Windows long path behavior:
  <https://learn.microsoft.com/en-us/windows/win32/fileio/maximum-file-path-limitation>
- Git environment variables:
  <https://git-scm.com/docs/git>
- Filesystem clone spike:
  [docs/design/windows-block-clone-spike.md](windows-block-clone-spike.md)

## Scope

The first native Windows release should support:

- NTFS correctness with byte-copy forks;
- ReFS block clone where a dedicated test volume proves it;
- Git for Windows as the shadow-git backend;
- Unicode paths, drive-letter paths, and UNC paths;
- file symlinks, directory symlinks, and ordinary directories/files;
- JSON output and hints that let agents self-correct.

It should not initially promise:

- network-share performance;
- exact ACL cloning across domains;
- junction/reparse-point traversal beyond explicit supported cases;
- performance parity with APFS/btrfs.

## Symlinks And Reparse Points

Windows distinguishes file symlinks and directory symlinks. Creating them may
require `SeCreateSymbolicLinkPrivilege`; unelevated creation can work only when
Developer Mode enables `SYMBOLIC_LINK_FLAG_ALLOW_UNPRIVILEGED_CREATE`.

Implementation rules:

- Detect reparse points without following them during checkpoint, fork, restore,
  or doctor scans.
- Preserve supported file and directory symlinks as links, not as target
  contents.
- Create directory symlinks with the directory flag and file symlinks without
  it.
- If symlink creation fails for privilege reasons, return
  `unsupported_platform` or a new Windows-specific error with a hint to enable
  Developer Mode, run elevated, or use WSL2.
- Treat junctions and mount points as unsupported until a separate design
  decides whether they are copied as reparse points, skipped, or blocked.

Required tests:

- fork preserves a relative file symlink;
- fork preserves a relative directory symlink;
- checkpoint/restore round-trips symlink metadata without following the target;
- symlink privilege failure has a JSON error code and actionable hint;
- doctor does not recurse through junctions or directory symlinks.

## Permissions And Attributes

Windows authorization is ACL-based. The Unix mode bits that `asp` currently
preserves are not enough to represent Windows ownership, inheritance, auditing,
or deny rules.

First-release behavior:

- Preserve file contents, readonly attributes, hidden/system attributes where
  safe, and modification times.
- Preserve symlink-ness before preserving detailed ACLs.
- Do not claim exact ACL fidelity in checkpoints or forks.
- Never weaken source permissions to make a copy succeed silently.
- Apply readonly attributes after file or directory contents are populated, not
  before.

Required tests:

- readonly files and directories fork successfully and remain readonly in the
  fork;
- hidden files are captured, restored, and forked;
- access-denied errors include the path role and next action;
- restore never writes outside the workspace when a stored path contains
  Windows separators, drive prefixes, or UNC-like input.

## Paths And Names

Windows path handling has several product-critical traps: drive-relative paths
like `C:tmp`, UNC paths, extended-length `\\?\` prefixes, reserved device names,
case-preserving but usually case-insensitive lookup, and long-path opt-in.

Implementation rules:

- Normalize user-supplied workspace roots to absolute paths before any store or
  git operation.
- Reject store paths with drive prefixes, UNC prefixes, absolute roots, `..`,
  or alternate data stream syntax.
- Keep checkpoint paths in portable slash-separated UTF-8 form only after the
  filesystem path has passed containment checks.
- Use wide Windows APIs for any native helper; do not round-trip through the
  process code page.
- Add a manifest or build setting for long-path awareness before declaring
  support for deeply nested enterprise repos.

Required tests:

- workspace root on `C:\...` works;
- workspace root under a UNC-like path is either supported or rejected with a
  specific hint;
- case-only renames restore correctly;
- `CON`, `PRN`, trailing-space, trailing-dot, alternate-stream, and `..` paths
  in store metadata are rejected;
- paths longer than 260 characters are either supported by manifest-backed long
  paths or rejected with a precise hint.

## Git For Windows

The shadow repo remains the recovery source of truth, so Git for Windows must be
isolated just as tightly as git on Unix.

Implementation rules:

- Require a minimum Git for Windows version and check it at runtime.
- Invoke git with explicit `GIT_DIR`, `GIT_WORK_TREE`, `GIT_INDEX_FILE`, and
  config isolation.
- Clear inherited `GIT_DIR`, `GIT_WORK_TREE`, `GIT_INDEX_FILE`,
  `GIT_OBJECT_DIRECTORY`, `GIT_ALTERNATE_OBJECT_DIRECTORIES`, and related
  variables before invoking user-git operations.
- Force shadow-git line-ending behavior so `core.autocrlf` from user/global
  config cannot mutate checkpoint bytes.
- Keep `promote` as the only path that writes ordinary refs into the user's
  repository.

Required tests:

- global `core.autocrlf=true` does not change shadow checkpoint bytes;
- inherited `GIT_DIR` and `GIT_WORK_TREE` do not affect checkpoint, restore,
  fork, or promote;
- Git for Windows safe-directory behavior does not block shadow-git recovery;
- `promote` creates a normal user branch and never stages `.asp/`.

## Release Gates

Native Windows support can replace `unsupported_platform` only after:

- all tests above pass on hosted Windows NTFS;
- ReFS clone behavior passes on a self-hosted or otherwise provisioned Windows
  Server runner;
- `docs/windows.md`, `docs/filesystems.md`, and release notes document the NTFS
  copy fallback honestly;
- `asp diagnostics` includes Windows version, filesystem type, git version, and
  symlink privilege status, redacted by default;
- a crash-safety pass proves interrupted fork/checkpoint/restore operations are
  doctor-repairable on Windows.
