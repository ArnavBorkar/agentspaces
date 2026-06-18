# Parser Fuzzing

agentspaces treats local files, MCP input, and hook payloads as untrusted. The
stable fuzz harnesses live in `crates/asp/tests/fuzz_harnesses.rs` and run as
part of the normal workspace test suite.

Covered parser surfaces:

- `.asp/config.toml` loading from arbitrary bytes.
- `.asp/journal.jsonl` reading from arbitrary bytes.
- MCP `tools/call` parameter validation over nested JSON values.
- Claude hook payload parsing after arbitrary bytes are accepted as JSON.

Run the default harness set:

```bash
cargo test -p asp --test fuzz_harnesses
```

Run a longer local campaign without changing source:

```bash
PROPTEST_CASES=5000 cargo test -p asp --test fuzz_harnesses
```

When a failure is found, proptest writes a regression seed next to the test. Keep
that seed in version control with the fix so CI preserves the minimized case.
