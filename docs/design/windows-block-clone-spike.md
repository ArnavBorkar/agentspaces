# Windows Block Clone Spike

This note records the Windows filesystem decision for native fork support. It
is intentionally conservative: `asp` should keep reporting Windows as
unsupported until these paths are implemented and tested on real ReFS/NTFS
volumes.

## Sources

- Microsoft ReFS block cloning:
  <https://learn.microsoft.com/en-us/windows-server/storage/refs/block-cloning>
- `FSCTL_DUPLICATE_EXTENTS_TO_FILE`:
  <https://learn.microsoft.com/en-us/windows/win32/api/winioctl/ni-winioctl-fsctl_duplicate_extents_to_file>
- `DUPLICATE_EXTENTS_DATA`:
  <https://learn.microsoft.com/en-us/windows/win32/api/winioctl/ns-winioctl-duplicate_extents_data>
- Windows filesystem feature comparison:
  <https://learn.microsoft.com/en-us/windows/win32/fileio/filesystem-functionality-comparison>
- `CopyFile2` fallback API:
  <https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-copyfile2>

## Findings

| Target | Decision | Reason |
| --- | --- | --- |
| ReFS on Windows Server 2016+ | Candidate for `block_clone` | Microsoft documents ReFS block cloning through `FSCTL_DUPLICATE_EXTENTS_TO_FILE`. |
| SMB 3.1.1 backed by a capable server | Candidate only after explicit probe | The FSCTL page lists SMB 3.1.1 support, but network behavior varies and is not a launch-critical path. |
| CsvFS | Candidate only after explicit probe | Supported by the FSCTL page, but cluster semantics need dedicated tests. |
| NTFS | Treat as `copy` | Microsoft documents many NTFS features, but local NTFS block cloning is not listed as supported for this FSCTL. |
| exFAT/FAT/UDF and removable media | Treat as `copy` or unsupported | Missing security/symlink semantics make these poor enterprise defaults. |

## Implementation Plan

Native Windows support should add a Windows-specific file clone helper with
this order:

1. Try ReFS block clone for regular files.
2. Fall back per file to `CopyFile2` or `std::fs::copy` when the FSCTL returns
   unsupported, invalid-parameter, access-denied, cross-volume, or remote
   capability errors.
3. Preserve correctness over speed. A failed clone attempt must delete any
   partial destination file before copy fallback.
4. Keep the current fork-level invariant: a fork either completes and is
   registered, or leaves only deterministic doctor-repairable state.

The ReFS path cannot be a blind whole-file syscall:

- source and destination ranges must be cluster-aligned;
- each duplicate-extent request must be less than 4 GiB;
- the destination length must be extended before cloning into it;
- sparse files require the destination to be sparse first;
- integrity-stream settings must match.

That means the helper needs a chunked algorithm:

1. Open the source for read and the destination for write.
2. Create/truncate the destination and set its final length.
3. Discover the volume cluster size.
4. Duplicate aligned chunks up to the largest valid multiple below 4 GiB.
5. Byte-copy any unaligned tail.
6. Apply readonly attributes and timestamps after file contents are complete.

`CloneMethod` should gain a Windows-facing value only when the user-visible
semantics are clear. Recommended first implementation:

- report `block_clone` only when every regular file was fully cloned through
  duplicate extents;
- report `copy` when any regular file used byte-copy fallback;
- optionally add diagnostics counters later for `files_block_cloned`,
  `files_copied`, and `bytes_copied`.

## Test Matrix

Before enabling native Windows:

- Windows Server 2022/2025 on ReFS: `asp --json fork` reports `block_clone` for
  a tree made of cluster-aligned regular files.
- Windows Server 2022/2025 on ReFS: a tree with odd-sized files succeeds,
  preserves bytes, and reports `copy` until mixed-method reporting exists.
- Windows 11/Server on NTFS: fork succeeds with `copy`.
- ReFS and NTFS: modifying a forked file never changes the parent.
- ReFS and NTFS: readonly directories, hidden files, large files over 4 GiB,
  sparse files, and long paths round-trip.
- ReFS and NTFS: kill-9 or process termination during fork leaves only state
  that `asp doctor --fix` can explain and repair.

## Product Decision

Do not market native Windows fast forks until ReFS block clone is implemented,
CI proves the ReFS path, and NTFS copy fallback is explicitly documented.
Enterprise users should see predictable correctness first, then filesystem
acceleration where the platform proves it.
