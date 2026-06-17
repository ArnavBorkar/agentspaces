# Filesystem Feature Detection

`asp fork` clones the whole physical workspace tree. The method depends on the
platform and the filesystem under the workspace root:

| Platform / filesystem | Expected fork method | Notes |
| --- | --- | --- |
| macOS APFS | `clonefile` | Fast whole-directory copy-on-write through `clonefile(2)`. |
| Linux btrfs | `reflink` | Per-file `FICLONE`; CI asserts this path on a btrfs loopback volume. |
| Linux XFS | `reflink` when enabled | XFS must be formatted with reflink support; otherwise `asp` falls back to copy. |
| Linux ext4 | `copy` | ext4 does not provide the reflink path used by `asp` today. |
| Linux tmpfs | `copy` | Useful for tests, but no durable CoW sharing. |
| Network or virtual filesystems | `copy` or error | NFS, SMB, Docker bind mounts, and cloud sync folders vary; probe before relying on CoW. |

The source of truth is the JSON output from a probe fork:

```bash
mkdir -p /path/on/the/filesystem/probe
cd /path/on/the/filesystem/probe
asp init
echo hello > file.txt
asp checkpoint -m base
asp --json fork --name fs-probe
asp discard fs-probe
```

Look at `result[0].method`:

- `clonefile` means macOS APFS CoW was used;
- `reflink` means Linux FICLONE CoW was used for every regular file;
- `copy` means the fork is still correct, but bytes were copied.

To identify the mounted filesystem before probing:

```bash
# Linux
findmnt -T "$PWD" -no FSTYPE,SOURCE,OPTIONS

# macOS
diskutil info "$PWD"
```

## Operational Guidance

- Keep forks on the same local volume as the workspace. `asp` creates fork
  siblings (`<repo>@<fork>`) specifically so CoW has a chance to work.
- Treat network filesystems as copy fallback until a probe proves otherwise.
- For performance-sensitive CI, put `TMPDIR` on the filesystem you want to
  exercise; the btrfs CI job does this before running the torture suite.
- Large-file CAS restores use the same file-level CoW helper where available,
  then fall back to byte copy.
- If `asp --json fork` reports `copy` on a filesystem where you expected CoW,
  include `findmnt`/`diskutil` output and `asp diagnostics` in the issue.
