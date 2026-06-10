#!/usr/bin/env python3
"""Generate a synthetic monorepo for fork/status/checkpoint benchmarks.

Shape modeled on a real large monorepo:
  - many packages with nested src dirs of small text files (source-like, compressible)
  - a handful of large incompressible binary assets (the hard case for capture)
  - a node_modules-like noise directory of tiny files

Deterministic given --seed. Prints a summary line at the end.
"""

import argparse
import os
import random
import sys
import time

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


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--root", required=True)
    ap.add_argument("--files", type=int, default=100_000)
    ap.add_argument("--blob-gb", type=float, default=3.0)
    ap.add_argument("--blob-count", type=int, default=24)
    ap.add_argument("--seed", type=int, default=42)
    args = ap.parse_args()

    rng = random.Random(args.seed)
    root = os.path.abspath(args.root)
    if os.path.exists(root):
        sys.exit(f"refusing to overwrite existing {root}")
    t0 = time.time()

    n_noise = args.files // 5  # node_modules-like tiny files
    n_src = args.files - n_noise

    # Source tree: 200 packages, nested modules.
    per_pkg = max(1, n_src // 200)
    written = 0
    for pkg in range(200):
        for i in range(per_pkg):
            if written >= n_src:
                break
            mod = i // 50
            d = os.path.join(root, f"packages/pkg{pkg:03d}/src/mod{mod:02d}")
            os.makedirs(d, exist_ok=True)
            body = SRC_TEMPLATE.format(mod=mod, idx=i)
            # Vary size 1–20 KB like real source files.
            body += "// padding\n" * rng.randint(0, 600)
            with open(os.path.join(d, f"file_{i:05d}.rs"), "w") as f:
                f.write(body)
            written += 1

    # Noise: tiny json/js files in a deep node_modules-ish tree.
    for i in range(n_noise):
        d = os.path.join(root, f"vendor/node_modules/dep{i % 500:03d}/dist")
        os.makedirs(d, exist_ok=True)
        with open(os.path.join(d, f"chunk_{i:05d}.js"), "w") as f:
            f.write(f"module.exports={{id:{i},x:'{rng.random()}'}};\n")

    # Large incompressible assets.
    blob_bytes = int(args.blob_gb * (1 << 30))
    per_blob = blob_bytes // args.blob_count
    os.makedirs(os.path.join(root, "assets"), exist_ok=True)
    for b in range(args.blob_count):
        with open(os.path.join(root, f"assets/blob_{b:02d}.bin"), "wb") as f:
            remaining = per_blob
            while remaining > 0:
                chunk = min(remaining, 1 << 24)
                f.write(os.urandom(chunk))
                remaining -= chunk

    with open(os.path.join(root, ".gitignore"), "w") as f:
        f.write("build/\n*.log\n")
    with open(os.path.join(root, "README.md"), "w") as f:
        f.write("# synthetic monorepo for asp benchmarks\n")

    total_files = sum(len(fs) for _, _, fs in os.walk(root))
    total_bytes = sum(
        os.path.getsize(os.path.join(dp, fn))
        for dp, _, fns in os.walk(root)
        for fn in fns
    )
    print(
        f"generated {total_files} files, {total_bytes / (1 << 30):.2f} GiB "
        f"in {time.time() - t0:.1f}s at {root}"
    )


if __name__ == "__main__":
    main()
