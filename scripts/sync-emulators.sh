#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

export PATH="${HOME}/.cargo/bin:${PATH}"

cargo test -p asp-core --test sync_emulators -- --ignored --nocapture
