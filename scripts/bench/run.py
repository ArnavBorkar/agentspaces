#!/usr/bin/env python3
"""Reproducible asp benchmark harness.

Generates a synthetic fixture (scripts/spike/gen_tree.py), then measures asp's
core operations against cp -R and git-worktree baselines. Emits a markdown
report to stdout.

Usage: python3 scripts/bench/run.py [--fixture monorepo] [--files 100000] [--blob-gb 3] [--keep]
Requires: a release build at target/release/asp.
"""

import argparse
import json
import os
import platform
import shutil
import subprocess
import sys
import tempfile
import time

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
ASP = os.path.join(REPO, "target", "release", "asp")
FIXTURES = ("monorepo", "small-files", "large-binaries", "deep-tree", "rename-heavy")
SKIP_DIRS = {".asp", ".git"}
TEXT_EXTS = {".cfg", ".js", ".json", ".md", ".rs", ".toml", ".txt", ".yaml", ".yml"}


def run(cmd, cwd=None, env=None):
    proc = subprocess.run(cmd, cwd=cwd, env=env, capture_output=True, text=True)
    if proc.returncode != 0:
        raise RuntimeError(
            f"command failed ({proc.returncode}): {' '.join(cmd)}\n"
            f"cwd: {cwd or os.getcwd()}\n"
            f"stdout:\n{proc.stdout}\n"
            f"stderr:\n{proc.stderr}"
        )
    return proc


def timed(fn):
    t0 = time.monotonic()
    out = fn()
    return (time.monotonic() - t0) * 1000, out


def asp(args, cwd):
    return run([ASP, *args], cwd=cwd)


def safe_join(root, rel):
    path = os.path.abspath(os.path.join(root, rel))
    root_abs = os.path.abspath(root)
    if path != root_abs and not path.startswith(root_abs + os.sep):
        raise RuntimeError(f"rename plan escapes fixture root: {rel}")
    return path


def text_edit_candidates(root):
    for dirpath, dirnames, filenames in os.walk(root):
        dirnames[:] = sorted(d for d in dirnames if d not in SKIP_DIRS)
        for filename in sorted(filenames):
            if filename == "rename-plan.tsv":
                continue
            path = os.path.join(dirpath, filename)
            rel = os.path.relpath(path, root)
            if rel == "README.md":
                continue
            ext = os.path.splitext(filename)[1]
            if ext in TEXT_EXTS:
                yield path


def append_incremental_edits(root, limit):
    edited = 0
    for path in text_edit_candidates(root):
        with open(path, "a") as f:
            f.write("// bench edit\n")
        edited += 1
        if edited >= limit:
            break
    if edited == 0:
        raise RuntimeError("fixture has no text files available for incremental edits")
    return edited


def apply_rename_workload(root, limit):
    plan = os.path.join(root, "rename-plan.tsv")
    if not os.path.exists(plan):
        raise RuntimeError("rename-heavy fixture is missing rename-plan.tsv")

    renamed = 0
    with open(plan) as f:
        for line in f:
            if renamed >= limit:
                break
            src_rel, dst_rel = line.rstrip("\n").split("\t", 1)
            src = safe_join(root, src_rel)
            dst = safe_join(root, dst_rel)
            if not os.path.exists(src):
                continue
            os.makedirs(os.path.dirname(dst), exist_ok=True)
            os.rename(src, dst)
            with open(dst, "a") as out:
                out.write("// bench rename edit\n")
            renamed += 1

    if renamed == 0:
        raise RuntimeError("rename-heavy fixture has no rename-plan entries to apply")
    return renamed


def mutate_for_incremental_checkpoint(root, fixture, limit):
    if fixture == "rename-heavy":
        renamed = apply_rename_workload(root, limit)
        return f"incremental checkpoint ({renamed} renames)"

    edited = append_incremental_edits(root, limit)
    return f"incremental checkpoint ({edited} edits)"


def parse_args():
    ap = argparse.ArgumentParser()
    ap.add_argument("--fixture", choices=FIXTURES, default="monorepo")
    ap.add_argument("--files", type=int, default=100_000)
    ap.add_argument("--blob-gb", type=float, default=3.0)
    ap.add_argument("--blob-count", type=int, default=24)
    ap.add_argument("--edit-count", type=int, default=10)
    ap.add_argument("--keep", action="store_true")
    args = ap.parse_args()
    if args.edit_count < 1:
        ap.error("--edit-count must be at least 1")
    return args


def main():
    args = parse_args()

    if not os.path.exists(ASP):
        sys.exit("build first: cargo build --release")

    base = os.environ.get("ASP_BENCH_BASE") or tempfile.mkdtemp(prefix="asp-bench-")
    tree = os.path.join(base, "bench-tree")
    if os.path.isdir(base):
        leftovers = [tree] + [
            os.path.join(base, d) for d in os.listdir(base) if d.startswith("bench-tree@")
        ]
        for leftover in leftovers:
            shutil.rmtree(leftover, ignore_errors=True)
    os.makedirs(base, exist_ok=True)

    print("generating tree...", file=sys.stderr)
    gen = run([
        sys.executable,
        os.path.join(REPO, "scripts", "spike", "gen_tree.py"),
        "--root", tree,
        "--fixture", args.fixture,
        "--files", str(args.files),
        "--blob-gb", str(args.blob_gb),
        "--blob-count", str(args.blob_count),
    ])
    gen_summary = gen.stdout.strip().splitlines()[-1]
    results = {}

    results["init"], _ = timed(lambda: asp(["init"], tree))
    results["first checkpoint (sidecar active)"], _ = timed(
        lambda: asp(["checkpoint", "-m", "initial"], tree)
    )

    incremental_label = mutate_for_incremental_checkpoint(tree, args.fixture, args.edit_count)
    results[incremental_label], _ = timed(
        lambda: asp(["checkpoint", "-m", "incremental"], tree)
    )
    results["no-op checkpoint"], _ = timed(lambda: asp(["checkpoint"], tree))
    results["status"], _ = timed(lambda: asp(["status"], tree))

    results["fork (whole tree, CoW)"], _ = timed(lambda: asp(["fork", "--name", "bench"], tree))
    results["forks compare"], _ = timed(lambda: asp(["forks"], tree))

    # Baselines.
    cp_dst = os.path.join(base, "cp-baseline")
    shutil.rmtree(cp_dst, ignore_errors=True)
    results["cp -R baseline"], _ = timed(
        lambda: run(["cp", "-R", tree, cp_dst])
    )
    shutil.rmtree(cp_dst, ignore_errors=True)

    git_env = {**os.environ, "GIT_CONFIG_GLOBAL": "/dev/null"}
    run(["git", "init", "-q"], cwd=tree, env=git_env)
    run(["git", "config", "user.email", "b@b"], cwd=tree, env=git_env)
    run(["git", "config", "user.name", "b"], cwd=tree, env=git_env)
    with open(os.path.join(tree, ".git/info/exclude"), "a") as f:
        f.write("/.asp/\n/assets/\n/vendor/\n")
    run(["git", "add", "-A"], cwd=tree, env=git_env)
    run(["git", "commit", "-qm", "baseline"], cwd=tree, env=git_env)
    wt = os.path.join(base, "wt-baseline")
    results["git worktree add baseline (tracked only)"], _ = timed(
        lambda: run(["git", "worktree", "add", "--detach", wt], cwd=tree, env=git_env)
    )
    run(["git", "worktree", "remove", "--force", wt], cwd=tree, env=git_env)

    store = subprocess.check_output(["du", "-sh", os.path.join(tree, ".asp")]).decode().split()[0]

    # Report.
    print(f"# asp benchmarks\n")
    print(f"*{time.strftime('%Y-%m-%d')} · {platform.platform()} · {platform.processor() or platform.machine()}*\n")
    print(f"Fixture: `{args.fixture}`\n")
    print(f"Tree: {gen_summary}\n")
    print("| Operation | Time |")
    print("|---|---:|")
    for name, ms in results.items():
        t = f"{ms/1000:.1f} s" if ms >= 2000 else f"{ms:.0f} ms"
        print(f"| {name} | {t} |")
    print(f"\n`.asp` store size after benchmarks: {store}")
    print("\nMethodology: every number is one cold run of the release binary via subprocess "
          "(includes process startup, ~5ms). Generator and harness are in `scripts/`; "
          "reproduce with `python3 scripts/bench/run.py`.")

    if not args.keep:
        shutil.rmtree(base, ignore_errors=True)


if __name__ == "__main__":
    main()
