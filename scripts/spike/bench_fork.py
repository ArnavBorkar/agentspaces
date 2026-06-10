#!/usr/bin/env python3
"""Benchmark whole-directory fork strategies on macOS APFS.

Compares:
  1. clonefile(2) on the directory root — kernel-recursive CoW clone (the asp fast path)
  2. /bin/cp -cR        — userspace walk, per-file clonefile
  3. /bin/cp -R         — full byte copy (baseline)
  4. git worktree add   — only if the tree is a git repo (tracked files only)

Reports wall time and extra disk consumed (APFS-aware, via df delta).
"""

import argparse
import ctypes
import ctypes.util
import os
import shutil
import subprocess
import sys
import time

libc = ctypes.CDLL(ctypes.util.find_library("c"), use_errno=True)


def clonefile(src: str, dst: str) -> None:
    # int clonefile(const char *src, const char *dst, int flags)
    res = libc.clonefile(src.encode(), dst.encode(), ctypes.c_int(0))
    if res != 0:
        err = ctypes.get_errno()
        raise OSError(err, os.strerror(err), src)


def df_used_kb(path: str) -> int:
    out = subprocess.check_output(["df", "-k", path]).decode().splitlines()[1].split()
    return int(out[2])


def timed(label: str, fn, src: str, dst: str) -> None:
    if os.path.exists(dst):
        shutil.rmtree(dst)
    used_before = df_used_kb(os.path.dirname(src))
    t0 = time.monotonic()
    fn(src, dst)
    dt = time.monotonic() - t0
    used_after = df_used_kb(os.path.dirname(src))
    extra_mb = max(0, used_after - used_before) / 1024
    print(f"{label:24s} {dt*1000:10.1f} ms   extra disk {extra_mb:10.1f} MB")


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--tree", required=True)
    ap.add_argument("--keep", action="store_true", help="keep the clonefile fork for later spikes")
    args = ap.parse_args()
    src = os.path.abspath(args.tree)
    base = os.path.dirname(src)

    print(f"tree: {src}")
    timed("clonefile(dir)", clonefile, src, f"{base}/fork-clonefile")
    timed("cp -cR (per-file clone)", lambda s, d: subprocess.run(["cp", "-cR", s, d], check=True),
          src, f"{base}/fork-cpc")
    timed("cp -R (full copy)", lambda s, d: subprocess.run(["cp", "-R", s, d], check=True),
          src, f"{base}/fork-cpr")

    # Clean up the heavy ones; optionally keep the clonefile fork.
    shutil.rmtree(f"{base}/fork-cpr")
    shutil.rmtree(f"{base}/fork-cpc")
    if not args.keep:
        shutil.rmtree(f"{base}/fork-clonefile")

    if os.path.isdir(os.path.join(src, ".git")):
        t0 = time.monotonic()
        subprocess.run(["git", "-C", src, "worktree", "add", "--detach", f"{base}/fork-worktree"],
                       check=True, capture_output=True)
        print(f"{'git worktree add':24s} {(time.monotonic()-t0)*1000:10.1f} ms   (tracked files only)")
        subprocess.run(["git", "-C", src, "worktree", "remove", "--force", f"{base}/fork-worktree"],
                       check=True, capture_output=True)
    else:
        print("git worktree:            skipped (tree is not a git repo yet)")


if __name__ == "__main__":
    main()
