#!/usr/bin/env python3
"""Spike: shadow-git capture layer.

Proves/measures, against the synthetic monorepo:
  1. A sidecar GIT_DIR (.asp/shadow.git) can capture the ENTIRE worktree
     (untracked files included) without a user-visible .git or touching one.
  2. Initial capture cost (with/without compression, with/without big blobs).
  3. Incremental capture cost after small edits (the auto-checkpoint path).
  4. Stock-git recovery: restore a checkpoint into a fresh dir with plain git
     commands only, byte-identical.
  5. Coexistence: a user git repo in the same tree stays untouched.

Usage: bench_shadow_git.py --tree bench-data/tree
"""

import argparse
import os
import subprocess
import time

ENV_KEEP = {"PATH", "HOME", "USER", "TMPDIR"}


def sh(cmd, env=None, cwd=None):
    e = {k: v for k, v in os.environ.items() if k in ENV_KEEP}
    if env:
        e.update(env)
    return subprocess.run(cmd, env=e, cwd=cwd, check=True, capture_output=True, text=True)


def timed(label, fn):
    t0 = time.monotonic()
    out = fn()
    dt = time.monotonic() - t0
    print(f"{label:44s} {dt*1000:10.1f} ms")
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--tree", required=True)
    ap.add_argument("--compression", type=int, default=0)
    args = ap.parse_args()
    tree = os.path.abspath(args.tree)
    gitdir = os.path.join(tree, ".asp", "shadow.git")
    genv = {
        "GIT_DIR": gitdir,
        "GIT_WORK_TREE": tree,
        "GIT_INDEX_FILE": os.path.join(tree, ".asp", "shadow.index"),
        "GIT_AUTHOR_NAME": "asp", "GIT_AUTHOR_EMAIL": "asp@local",
        "GIT_COMMITTER_NAME": "asp", "GIT_COMMITTER_EMAIL": "asp@local",
    }

    # --- 1. init shadow repo ---
    os.makedirs(gitdir, exist_ok=True)
    sh(["git", "init", "--bare", "-q", gitdir])
    sh(["git", "config", "core.compression", str(args.compression)], env=genv)
    sh(["git", "config", "gc.auto", "0"], env=genv)
    sh(["git", "config", "core.untrackedCache", "true"], env=genv)
    with open(os.path.join(gitdir, "info", "exclude"), "w") as f:
        f.write("/.asp/\n")
    print(f"shadow repo at {gitdir} (compression={args.compression})")

    # --- 2. initial capture ---
    # Respect .gitignore (build/, *.log ignored), capture everything else incl. untracked.
    def initial_add():
        sh(["git", "add", "-A", "."], env=genv, cwd=tree)
    timed("initial add -A (100k files, 3.3GiB)", initial_add)
    tree_oid = timed("write-tree", lambda: sh(["git", "write-tree"], env=genv).stdout.strip())
    commit = timed(
        "commit-tree",
        lambda: sh(["git", "commit-tree", tree_oid, "-m", "checkpoint 0"], env=genv).stdout.strip(),
    )
    sh(["git", "update-ref", "refs/asp/checkpoints/c0", commit], env=genv)

    # --- 3. incremental capture after touching 10 files ---
    for i in range(10):
        p = os.path.join(tree, f"packages/pkg{i:03d}/src/mod00/file_00000.rs")
        with open(p, "a") as f:
            f.write(f"// edit {time.time()}\n")
    with open(os.path.join(tree, "packages/pkg000/new_untracked.rs"), "w") as f:
        f.write("// brand new untracked file\n")

    def incremental():
        sh(["git", "add", "-A", "."], env=genv, cwd=tree)
        t = sh(["git", "write-tree"], env=genv).stdout.strip()
        c = sh(["git", "commit-tree", t, "-p", commit, "-m", "checkpoint 1"], env=genv).stdout.strip()
        sh(["git", "update-ref", "refs/asp/checkpoints/c1", c], env=genv)
        return c
    c1 = timed("incremental checkpoint (11 changes)", incremental)

    def incremental_noop():
        sh(["git", "add", "-A", "."], env=genv, cwd=tree)
        return sh(["git", "write-tree"], env=genv).stdout.strip()
    timed("no-op rescan (nothing changed)", incremental_noop)

    # --- 4. stock-git recovery into a fresh dir ---
    restore = os.path.join(os.path.dirname(tree), "restore-test")
    os.makedirs(restore, exist_ok=True)

    def do_restore():
        renv = dict(genv)
        renv["GIT_WORK_TREE"] = restore
        renv["GIT_INDEX_FILE"] = os.path.join(restore, ".restore.index")
        sh(["git", "read-tree", c1], env=renv)
        sh(["git", "checkout-index", "-a", "-f"], env=renv)
    timed("restore checkpoint c1 to fresh dir", do_restore)

    diff = subprocess.run(
        ["diff", "-r", "-q", "-x", ".asp", "-x", ".restore.index", "-x", "build", tree, restore],
        capture_output=True, text=True,
    )
    ok = diff.returncode == 0
    print(f"byte-identical restore (diff -r): {'PASS' if ok else 'FAIL'}")
    if not ok:
        print(diff.stdout[-2000:])

    # --- 5. user git repo coexistence + worktree baseline ---
    sh(["git", "init", "-q", "."], cwd=tree)
    sh(["git", "config", "user.email", "u@local"], cwd=tree)
    sh(["git", "config", "user.name", "u"], cwd=tree)
    with open(os.path.join(tree, ".git", "info", "exclude"), "a") as f:
        f.write("/.asp/\nvendor/\nassets/\n")
    sh(["git", "add", "packages", "README.md"], cwd=tree)
    sh(["git", "commit", "-qm", "user commit"], cwd=tree)
    t0 = time.monotonic()
    sh(["git", "worktree", "add", "--detach", os.path.join(os.path.dirname(tree), "fork-worktree")], cwd=tree)
    print(f"{'git worktree add (tracked only)':44s} {(time.monotonic()-t0)*1000:10.1f} ms")
    sh(["git", "worktree", "remove", "--force", os.path.join(os.path.dirname(tree), "fork-worktree")], cwd=tree)

    # shadow capture still works with user .git present (git auto-ignores .git dirs)
    def incremental_with_usergit():
        sh(["git", "add", "-A", "."], env=genv, cwd=tree)
        return sh(["git", "write-tree"], env=genv).stdout.strip()
    timed("shadow rescan with user .git present", incremental_with_usergit)
    user_log = sh(["git", "log", "--oneline"], cwd=tree).stdout.strip()
    print(f"user repo intact: {'PASS' if user_log.endswith('user commit') else 'FAIL'} ({user_log})")

    du = subprocess.check_output(["du", "-sh", gitdir]).decode().split()[0]
    print(f"shadow store size: {du}")


if __name__ == "__main__":
    main()
