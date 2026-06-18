# Windows Filesystem Capabilities

Native Windows support is still disabled, but enterprise evaluators need to
know which Windows-adjacent storage layouts are safe today and which ones are
future native targets.

Use this page to choose a pilot layout before running agents on a large repo.
The short version: WSL2 on the Linux filesystem is the supported Windows-hosted
path today; native NTFS and ReFS remain future work; network and cloud-synced
folders need explicit probes before anyone depends on fork performance.

## Capability Matrix

| Layout | Status today | Expected fork method | Guidance |
| --- | --- | --- | --- |
| WSL2 Linux filesystem, for example `~/repo` inside the distribution | Supported through the Linux binary | `copy` on ext4 unless the distro is backed by a reflink-capable filesystem | Best Windows-hosted pilot path today. Keep the repo inside the WSL2 filesystem, not under `/mnt/c`. |
| WSL2 access to Windows drives, for example `/mnt/c/Users/...` | Not recommended for active workspaces | Usually `copy` or slower metadata behavior | Use only for short experiments. Path case behavior, symlink behavior, and file notification semantics differ from normal Linux filesystems. |
| Native NTFS | Future native target | `copy` | Correct byte-copy forks should land before any native Windows performance claim. Preserve readonly/hidden attributes and symlink intent before claiming support. |
| Native ReFS on Windows Server | Future native acceleration target | `block_clone` only after a verified probe | ReFS is the candidate for fast forks through duplicate extents. It needs dedicated CI or self-hosted runner coverage before launch. |
| SMB or other network shares | Future explicit-probe target | `copy`, error, or provider-specific clone | Do not assume server-side cloning. Treat latency, locking, offline files, and antivirus scanning as rollout risks. |
| OneDrive, Dropbox, Google Drive, or similar synced folders | Not recommended for live `.asp/` stores | `copy` or error | Use `asp sync` or endpoint backup for durability instead of putting active workspaces inside a sync client. |

## WSL2 Layout

For Windows users evaluating `asp` today, install and run the Linux build inside
WSL2:

```bash
cd ~
git clone <repo-url> repo
cd repo
asp init
asp checkpoint -m baseline
```

Keep the workspace under the Linux distribution filesystem, such as
`/home/<user>/repo`. Avoid `/mnt/c`, `/mnt/d`, or other DrvFS mounts for agent
workloads unless a small probe already proved the exact repo behaves well.

Why this matters:

- Linux path and symlink semantics match the tested `asp` engine.
- Fork siblings stay inside the same WSL2 filesystem.
- Windows Defender, sync clients, and Explorer metadata writes are less likely
  to race active agent work.
- The native Windows `unsupported_platform` guard does not apply because the
  Linux binary is running on Linux.

## Native NTFS

Native NTFS support should prioritize correctness over speed:

- fork should fall back to byte copy unless a future Windows API probe proves a
  safe clone path;
- readonly files and directories should be populated first, then attributes
  reapplied;
- hidden/system attributes should be preserved where doing so does not weaken
  safety;
- supported file and directory symlinks should stay links;
- junctions and mount-point reparse points should be rejected with a corrective
  hint until a separate design supports them.

Do not promise copy-on-write forks on NTFS until CI proves it. The first native
NTFS release can still be valuable if it is predictable, recoverable, and clear
about copy costs.

## Native ReFS

ReFS is the candidate for native fast forks. The planned acceleration path is
duplicate extents, surfaced in `asp` only after a verified filesystem probe.

The probe must prove:

- source and destination are on the same ReFS volume;
- regular files can be duplicated without changing parent bytes when a fork is
  edited;
- unaligned tails, sparse files, readonly files, hidden files, and files larger
  than 4 GiB behave correctly;
- failure falls back to copy or stops with a hint before leaving partial output;
- kill or process termination during fork leaves state that `asp doctor --fix`
  can explain and repair.

Until that exists, ReFS should be documented as a future target, not as a
supported fast path.

## Network Shares

Network shares are not one filesystem. SMB server version, offline-files
settings, antivirus policy, permissions, and latency can all change behavior.

For enterprise pilots:

- keep active agent work on local storage when possible;
- use network storage for backups, artifacts, or explicit `asp sync` remotes
  rather than live fork roots;
- run a disposable probe on the exact share before any rollout;
- record share type, server version, offline-files status, and observed fork
  method in support tickets.

If a future SMB path can safely use server-side duplicate extents, it should be
opt-in behind the same verified-probe standard as ReFS.

## Cloud-Synced Folders

Do not put active `.asp/` workspaces inside consumer or enterprise sync-client
folders by default. Sync clients can hydrate placeholders, rewrite metadata,
hold locks, rename conflict files, or race a checkpoint.

Recommended layout:

```text
local fast disk:
  ~/work/repo/
  ~/work/repo/.asp/

durability:
  endpoint backup of the repo and .asp
  explicit asp sync remote
  normal git remote for promoted branches
```

Use [backup and disaster recovery](backup-recovery.md) and
[sync remote recovery](sync-recovery.md) for durability instead of relying on a
desktop sync client to preserve an active workspace consistently.

## Probe Checklist

Before approving a Windows-hosted layout:

- Run `asp bench self --json` or the filesystem probe from
  [filesystem detection](filesystems.md).
- Inspect `prerequisites[]` for `platform.supported`,
  `filesystem.symlinks`, `filesystem.hardlinks`,
  `filesystem.atomic_rename`, and `fork.copy_on_write`.
- On native Windows, record whether the symlink prerequisite says Developer
  Mode or `SeCreateSymbolicLinkPrivilege` is available for the path.
- Record whether symlink support is available.
- Record the observed fork method: `copy`, `reflink`, `clonefile`, or future
  `block_clone`.
- Create, checkpoint, fork, restore, and discard a disposable repo.
- Include native path, WSL2 path, filesystem type, and whether the repo lives on
  local, network, or synced storage in rollout notes.

## Related Docs

- [Windows status](windows.md)
- [Filesystem feature detection](filesystems.md)
- [Windows block clone spike](design/windows-block-clone-spike.md)
- [Native Windows support plan](design/windows-support-plan.md)
- [Backup and disaster recovery](backup-recovery.md)
