//! Stable parser fuzz harnesses.
//!
//! These run under `cargo test` so CI continuously exercises untrusted parser
//! inputs without requiring nightly or cargo-fuzz.

use asp::hooks;
use asp::mcp;
use asp_core::config::Config;
use asp_core::journal::Journal;
use proptest::prelude::*;
use serde_json::{Map, Value};

fn json_value() -> BoxedStrategy<Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        any::<i64>().prop_map(|n| Value::Number(n.into())),
        "[ -~]{0,64}".prop_map(Value::String),
    ];

    leaf.prop_recursive(4, 64, 8, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..8).prop_map(Value::Array),
            prop::collection::btree_map("[A-Za-z_][A-Za-z0-9_]{0,15}", inner, 0..8).prop_map(
                |values| {
                    let mut object = Map::new();
                    for (key, value) in values {
                        object.insert(key, value);
                    }
                    Value::Object(object)
                },
            ),
        ]
    })
    .boxed()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn fuzz_config_parser_never_panics(bytes in prop::collection::vec(any::<u8>(), 0..4096)) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, bytes).unwrap();

        let _ = Config::load(&path);
    }

    #[test]
    fn fuzz_journal_reader_never_panics(bytes in prop::collection::vec(any::<u8>(), 0..4096)) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("journal.jsonl");
        std::fs::write(&path, bytes).unwrap();

        let _ = Journal::new(path).read();
    }

    #[test]
    fn fuzz_mcp_tool_call_params_never_panics(value in json_value()) {
        let _ = mcp::parse_tool_call_params(&value);
    }

    #[test]
    fn fuzz_hook_payload_parser_never_panics(bytes in prop::collection::vec(any::<u8>(), 0..4096)) {
        if let Ok(value) = serde_json::from_slice::<Value>(&bytes) {
            let _ = hooks::parse_hook_payload(&value);
        }
    }
}
