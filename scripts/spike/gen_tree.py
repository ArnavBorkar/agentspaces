#!/usr/bin/env python3
"""Generate synthetic repository trees for asp benchmarks.

The default fixture is the original monorepo stress tree. Additional fixtures
target specific enterprise pain points: huge metadata fans, large binaries,
very deep paths, and rename-heavy change sets.

Tree layout is deterministic given --seed. Large blob contents are intentionally
random so they remain incompressible.
"""

import argparse
import os
import random
import sys
import time

FIXTURES = ("monorepo", "small-files", "large-binaries", "deep-tree", "rename-heavy")

SRC_TEMPLATE = """// module {mod} file {idx}
use std::collections::HashMap;

pub fn handler_{idx}(input: &str) -> Result<String, String> {{
    let mut state: HashMap<String, u64> = HashMap::new();
    for (i, tok) in input.split_whitespace().enumerate() {{
        *state.entry(tok.to_string()).or_insert(0) += i as u64;
    }}
    Ok(format!("{{}} tokens", state.len()))
}}

#[cfg(test)]
mod tests_{idx} {{
    #[test]
    fn smoke_{idx}() {{
        assert!(super::handler_{idx}("a b c").is_ok());
    }}
}}
"""


def ensure_parent(path: str) -> None:
    os.makedirs(os.path.dirname(path), exist_ok=True)


def write_text(path: str, body: str) -> None:
    ensure_parent(path)
    with open(path, "w") as f:
        f.write(body)


def write_random_blob(path: str, size: int) -> None:
    ensure_parent(path)
    with open(path, "wb") as f:
        remaining = size
        while remaining > 0:
            chunk = min(remaining, 1 << 24)
            f.write(os.urandom(chunk))
            remaining -= chunk


def blob_sizes(blob_gb: float, blob_count: int) -> list[int]:
    total = int(blob_gb * (1 << 30))
    base = total // blob_count
    remainder = total % blob_count
    return [base + (1 if i < remainder else 0) for i in range(blob_count)]


def generate_monorepo(root: str, args: argparse.Namespace, rng: random.Random) -> None:
    n_noise = args.files // 5
    n_src = args.files - n_noise

    written = 0
    file_idx = 0
    while written < n_src:
        for pkg in range(200):
            if written >= n_src:
                break
            mod = file_idx // 50
            body = SRC_TEMPLATE.format(mod=mod, idx=file_idx)
            body += "// padding\n" * rng.randint(0, 600)
            write_text(
                os.path.join(
                    root,
                    f"packages/pkg{pkg:03d}/src/mod{mod:02d}/file_{file_idx:05d}.rs",
                ),
                body,
            )
            written += 1
        file_idx += 1

    for i in range(n_noise):
        write_text(
            os.path.join(root, f"vendor/node_modules/dep{i % 500:03d}/dist/chunk_{i:05d}.js"),
            f"module.exports={{id:{i},x:'{rng.random()}'}};\n",
        )

    for b, size in enumerate(blob_sizes(args.blob_gb, args.blob_count)):
        write_random_blob(os.path.join(root, f"assets/blob_{b:02d}.bin"), size)


def generate_small_files(root: str, args: argparse.Namespace, rng: random.Random) -> None:
    for i in range(args.files):
        body = (
            f"id={i}\n"
            f"tenant=team-{i % 37:02d}\n"
            f"checksum={rng.randrange(1 << 32):08x}\n"
        )
        write_text(
            os.path.join(
                root,
                f"tiny-files/team-{i % 37:02d}/bucket-{i % 251:03d}/record_{i:07d}.txt",
            ),
            body,
        )


def generate_large_binaries(root: str, args: argparse.Namespace, rng: random.Random) -> None:
    metadata_files = max(1, args.files - args.blob_count)
    sizes = blob_sizes(args.blob_gb, args.blob_count)

    for i in range(metadata_files):
        blob = i % args.blob_count
        write_text(
            os.path.join(root, f"asset-manifests/group-{i % 32:02d}/asset_{i:06d}.toml"),
            "\n".join(
                [
                    f"id = {i}",
                    f'blob = "large-binaries/shard-{blob % 16:02d}/blob_{blob:04d}.bin"',
                    f'checksum_hint = "{rng.randrange(1 << 64):016x}"',
                    "",
                ]
            ),
        )

    for b, size in enumerate(sizes):
        write_random_blob(
            os.path.join(root, f"large-binaries/shard-{b % 16:02d}/blob_{b:04d}.bin"),
            size,
        )


def generate_deep_tree(root: str, args: argparse.Namespace, rng: random.Random) -> None:
    for i in range(args.files):
        depth = 8 + (i % 41)
        layers = [f"layer{level:02d}_{(i + level) % 17:02d}" for level in range(depth)]
        write_text(
            os.path.join(root, "deep-tree", *layers, f"leaf_{i:06d}.rs"),
            SRC_TEMPLATE.format(mod=depth, idx=i) + ("// path padding\n" * rng.randint(0, 8)),
        )


def generate_rename_heavy(root: str, args: argparse.Namespace, rng: random.Random) -> None:
    plan_rows = []
    for i in range(args.files):
        src_rel = f"rename-workload/current/shard-{i % 64:02d}/item_{i:06d}.rs"
        dst_rel = f"rename-workload/moved/component-{i % 23:02d}/item_{i:06d}.rs"
        body = SRC_TEMPLATE.format(mod=i % 50, idx=i)
        body += f"// rename cohort {rng.randrange(1 << 32):08x}\n"
        write_text(os.path.join(root, src_rel), body)
        plan_rows.append(f"{src_rel}\t{dst_rel}\n")

    write_text(os.path.join(root, "rename-plan.tsv"), "".join(plan_rows))


def write_common_files(root: str, fixture: str) -> None:
    write_text(os.path.join(root, ".gitignore"), "build/\n*.log\n")
    write_text(
        os.path.join(root, "README.md"),
        f"# synthetic {fixture} fixture for asp benchmarks\n",
    )


def parse_args() -> argparse.Namespace:
    ap = argparse.ArgumentParser()
    ap.add_argument("--root", required=True)
    ap.add_argument("--fixture", choices=FIXTURES, default="monorepo")
    ap.add_argument("--files", type=int, default=100_000)
    ap.add_argument("--blob-gb", type=float, default=3.0)
    ap.add_argument("--blob-count", type=int, default=24)
    ap.add_argument("--seed", type=int, default=42)
    args = ap.parse_args()

    if args.files < 1:
        ap.error("--files must be at least 1")
    if args.blob_gb < 0:
        ap.error("--blob-gb must not be negative")
    if args.blob_count < 1:
        ap.error("--blob-count must be at least 1")

    return args


def main() -> None:
    args = parse_args()

    rng = random.Random(args.seed)
    root = os.path.abspath(args.root)
    if os.path.exists(root):
        sys.exit(f"refusing to overwrite existing {root}")
    t0 = time.time()

    generators = {
        "monorepo": generate_monorepo,
        "small-files": generate_small_files,
        "large-binaries": generate_large_binaries,
        "deep-tree": generate_deep_tree,
        "rename-heavy": generate_rename_heavy,
    }
    generators[args.fixture](root, args, rng)
    write_common_files(root, args.fixture)

    total_files = sum(len(fs) for _, _, fs in os.walk(root))
    total_bytes = sum(
        os.path.getsize(os.path.join(dp, fn))
        for dp, _, fns in os.walk(root)
        for fn in fns
    )
    print(
        f"generated {args.fixture} fixture: {total_files} files, "
        f"{total_bytes / (1 << 30):.2f} GiB in {time.time() - t0:.1f}s at {root}"
    )


if __name__ == "__main__":
    main()
