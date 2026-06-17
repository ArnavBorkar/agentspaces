#!/usr/bin/env python3
"""Reproducible asp benchmark harness.

Generates the synthetic monorepo (scripts/spike/gen_tree.py), then measures
asp's core operations against cp -R and git-worktree baselines. Emits a
markdown report to stdout.

Usage: python3 scripts/bench/run.py [--files 100000] [--blob-gb 3] [--keep]
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


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--files", type=int, default=100_000)
    ap.add_argument("--blob-gb", type=float, default=3.0)
    ap.add_argument("--keep", action="store_true")
    args = ap.parse_args()

    if not os.path.exists(ASP):
        sys.exit("build first: cargo build --release")

    base = os.environ.get("ASP_BENCH_BASE") or tempfile.mkdtemp(prefix="asp-bench-")
    tree = os.path.join(base, "bench-tree")
    for leftover in [tree] + [
        os.path.join(base, d) for d in os.listdir(base) if d.startswith("bench-tree@")
    ] if os.path.isdir(base) else []:
        shutil.rmtree(leftover, ignore_errors=True)
    os.makedirs(base, exist_ok=True)

    print("generating tree…", file=sys.stderr)
    gen = run([
        sys.executable,
        os.path.join(REPO, "scripts", "spike", "gen_tree.py"),
        "--root", tree, "--files", str(args.files), "--blob-gb", str(args.blob_gb),
    ])
    gen_summary = gen.stdout.strip().splitlines()[-1]
    results = {}

    results["init"], _ = timed(lambda: asp(["init"], tree))
    results["first checkpoint (sidecar active)"], _ = timed(
        lambda: asp(["checkpoint", "-m", "initial"], tree)
    )

    # Incremental: touch 10 source files.
    for i in range(10):
        p = os.path.join(tree, f"packages/pkg{i:03d}/src/mod00/file_00000.rs")
        with open(p, "a") as f:
            f.write("// bench edit\n")
    results["incremental checkpoint (10 edits)"], _ = timed(
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
