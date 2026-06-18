# Windows Status

Native Windows support is intentionally disabled in this release. Use WSL2,
macOS, or Linux for now.

The current blockers are practical filesystem semantics, not lack of interest:

- directory copy-on-write needs a tested ReFS/NTFS strategy;
- symlink creation and permission behavior differ from Unix defaults;
- path normalization, drive prefixes, and case behavior need dedicated tests;
- shadow-git environment isolation must be verified with Git for Windows.

The engine already keeps checkpoint store paths in a Windows-portable shape on
supported Unix hosts. UTF-8 paths that would become reserved device names
(`CON`, `NUL`, `COM1`, `LPT1`, and variants), alternate data streams,
backslash-separated paths, trailing-space/trailing-dot names, control-character
names, or overlong Windows components are rejected before they enter checkpoint
metadata. The corrective hint says `rename the path before checkpointing`.

The CLI reports `unsupported_platform` on Windows with a hint to use WSL2 and
track native support through the
[Windows issue list](https://github.com/ArnavBorkar/agentspaces/issues?q=is%3Aissue+label%3Awindows).

CI includes a Windows unsupported gate. It builds the workspace, runs the
Windows-specific unit guard, and checks that `asp init --json` exits nonzero with
the documented `unsupported_platform` code and WSL2 hint. Before native Windows
is marked supported, that job must be replaced by a real Windows behavior suite.

The first filesystem spike is documented in
[docs/design/windows-block-clone-spike.md](design/windows-block-clone-spike.md):
ReFS is the candidate for block cloning, while NTFS stays on byte-copy fallback
unless Microsoft documents and CI proves a safe local clone path.

The broader native support gate is documented in
[docs/design/windows-support-plan.md](design/windows-support-plan.md), covering
symlinks, permissions, paths, Git for Windows, diagnostics, and release tests.
