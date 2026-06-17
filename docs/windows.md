# Windows Status

Native Windows support is intentionally disabled in this release. Use WSL2,
macOS, or Linux for now.

The current blockers are practical filesystem semantics, not lack of interest:

- directory copy-on-write needs a tested ReFS/NTFS strategy;
- symlink creation and permission behavior differ from Unix defaults;
- path normalization, drive prefixes, and case behavior need dedicated tests;
- shadow-git environment isolation must be verified with Git for Windows.

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
