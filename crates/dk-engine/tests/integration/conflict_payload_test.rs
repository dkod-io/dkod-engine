use dk_engine::conflict::{build_conflict_block, build_conflict_detail};
use dk_engine::parser::ParserRegistry;

fn registry() -> ParserRegistry {
    ParserRegistry::new()
}

#[test]
fn test_payload_contains_base_their_your_versions() {
    let base = r#"fn process(x: i32) -> i32 {
    x + 1
}
"#;

    let theirs = r#"fn process(x: i32) -> i32 {
    x * 2
}
"#;

    let yours = r#"fn process(x: i32) -> i32 {
    x + 10
}
"#;

    let detail = build_conflict_detail(
        &registry(),
        "lib.rs",
        "process",
        "agent-other",
        base,
        theirs,
        yours,
    )
    .unwrap();

    assert_eq!(detail.file_path, "lib.rs");
    assert_eq!(detail.qualified_name, "process");
    assert_eq!(detail.kind, "function");
    assert_eq!(detail.conflicting_agent, "agent-other");

    // Base version present
    assert_eq!(detail.base_version.change_type, "base");
    assert!(detail.base_version.body.contains("x + 1"));

    // Their version present
    assert_eq!(detail.their_change.change_type, "modified");
    assert!(detail.their_change.body.contains("x * 2"));

    // Your version present
    assert_eq!(detail.your_change.change_type, "modified");
    assert!(detail.your_change.body.contains("x + 10"));
}

#[test]
fn test_payload_description_captures_signature_change() {
    let base = r#"fn compute(x: i32) -> i32 {
    x
}
"#;

    let theirs = r#"fn compute(x: i32, y: i32) -> i32 {
    x + y
}
"#;

    let yours = r#"fn compute(x: i32) -> i32 {
    x * 2
}
"#;

    let detail = build_conflict_detail(
        &registry(),
        "math.rs",
        "compute",
        "agent-b",
        base,
        theirs,
        yours,
    )
    .unwrap();

    // Their change should mention the signature change
    assert!(
        detail
            .their_change
            .description
            .contains("Signature changed"),
        "Expected signature change description, got: {}",
        detail.their_change.description
    );

    // Their signature should reflect the new params
    assert!(detail.their_change.signature.contains("y: i32"));
}

#[test]
fn test_payload_for_new_symbol() {
    // Symbol doesn't exist in base
    let base = r#"fn existing() -> bool {
    true
}
"#;

    let theirs = r#"fn existing() -> bool {
    true
}

fn new_fn() -> String {
    "hello".to_string()
}
"#;

    let yours = r#"fn existing() -> bool {
    true
}

fn new_fn() -> i32 {
    42
}
"#;

    let detail = build_conflict_detail(
        &registry(),
        "lib.rs",
        "new_fn",
        "agent-x",
        base,
        theirs,
        yours,
    )
    .unwrap();

    // Base version should be empty since the symbol didn't exist
    assert!(detail.base_version.body.is_empty());
    assert_eq!(detail.base_version.change_type, "base");
    assert!(detail.base_version.description.contains("does not exist"));

    // Both sides should be "added"
    assert_eq!(detail.their_change.change_type, "added");
    assert_eq!(detail.your_change.change_type, "added");
}

#[test]
fn test_build_conflict_block_message() {
    let base = r#"fn alpha() -> i32 { 1 }
"#;
    let theirs = r#"fn alpha() -> i32 { 2 }
"#;
    let yours = r#"fn alpha() -> i32 { 3 }
"#;

    let conflicts = vec![("src/lib.rs", "alpha", "agent-other", base, theirs, yours)];

    let block = build_conflict_block(&registry(), &conflicts).unwrap();
    assert_eq!(block.conflicting_symbols.len(), 1);
    assert!(block.message.contains("1 symbol conflict"));
    assert!(block.message.contains("src/lib.rs"));
}

#[test]
fn test_build_conflict_block_multiple_files() {
    let base_a = r#"fn fa() -> i32 { 1 }
"#;
    let theirs_a = r#"fn fa() -> i32 { 2 }
"#;
    let yours_a = r#"fn fa() -> i32 { 3 }
"#;

    let base_b = r#"fn fb() -> i32 { 1 }
"#;
    let theirs_b = r#"fn fb() -> i32 { 2 }
"#;
    let yours_b = r#"fn fb() -> i32 { 3 }
"#;

    let conflicts = vec![
        ("src/a.rs", "fa", "agent-1", base_a, theirs_a, yours_a),
        ("src/b.rs", "fb", "agent-2", base_b, theirs_b, yours_b),
    ];

    let block = build_conflict_block(&registry(), &conflicts).unwrap();
    assert_eq!(block.conflicting_symbols.len(), 2);
    assert!(block.message.contains("2 symbol conflicts"));
    assert!(block.message.contains("2 file(s)"));
}
