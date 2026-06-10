#!/bin/sh
# Prepare the demo directory and record docs/assets/demo.gif with vhs.
# Usage: scripts/make-demo.sh   (requires: vhs on PATH, asp on PATH)
set -eu

REPO="$(cd "$(dirname "$0")/.." && pwd)"
DEMO=/tmp/asp-gif-demo

rm -rf "$DEMO" /tmp/asp-gif-demo@* 2>/dev/null || true
mkdir -p "$DEMO"
cd "$DEMO"

cat > app.py << 'EOF'
def greet():
    return "hello"
EOF
printf '# demo project\n' > README.md

# promote needs a git repo
git init -q .
git -c user.email=demo@demo -c user.name=demo add -A
git -c user.email=demo@demo -c user.name=demo commit -qm init

cd "$REPO"
vhs docs/assets/demo.tape

rm -rf "$DEMO" /tmp/asp-gif-demo@* 2>/dev/null || true
echo "wrote docs/assets/demo.gif"
