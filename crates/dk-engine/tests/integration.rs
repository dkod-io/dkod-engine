// All integration tests consolidated into a single binary.
// Each file compiles tree-sitter grammars — with 15 languages and 32 test files,
// that's 480 redundant C compilations. One binary = one compilation.

#[path = "integration/ast_merge_test.rs"]
mod ast_merge_test;
#[path = "integration/eviction_recovery_test.rs"]
mod eviction_recovery_test;
#[path = "integration/bash_parser_test.rs"]
mod bash_parser_test;
#[path = "integration/changeset_state_test.rs"]
mod changeset_state_test;
#[path = "integration/conflict_claim_test.rs"]
mod conflict_claim_test;
#[path = "integration/conflict_payload_test.rs"]
mod conflict_payload_test;
#[path = "integration/cpp_parser_test.rs"]
mod cpp_parser_test;
#[path = "integration/csharp_parser_test.rs"]
mod csharp_parser_test;
#[path = "integration/git_test.rs"]
mod git_test;
#[path = "integration/go_parser_test.rs"]
mod go_parser_test;
#[path = "integration/graph_callgraph_test.rs"]
mod graph_callgraph_test;
#[path = "integration/graph_symbols_test.rs"]
mod graph_symbols_test;
#[path = "integration/haskell_parser_test.rs"]
mod haskell_parser_test;
#[path = "integration/java_parser_test.rs"]
mod java_parser_test;
#[path = "integration/julia_parser_test.rs"]
mod julia_parser_test;
#[path = "integration/kotlin_parser_test.rs"]
mod kotlin_parser_test;
#[path = "integration/nsi_git_ops_test.rs"]
mod nsi_git_ops_test;
#[path = "integration/nsi_merge_test.rs"]
mod nsi_merge_test;
#[path = "integration/nsi_workspace_test.rs"]
mod nsi_workspace_test;
#[path = "integration/parser_registry_test.rs"]
mod parser_registry_test;
#[path = "integration/php_parser_test.rs"]
mod php_parser_test;
#[path = "integration/python_parser_test.rs"]
mod python_parser_test;
#[path = "integration/repo_test.rs"]
mod repo_test;
#[path = "integration/ruby_parser_test.rs"]
mod ruby_parser_test;
#[path = "integration/rust_parser_test.rs"]
mod rust_parser_test;
#[path = "integration/scala_parser_test.rs"]
mod scala_parser_test;
#[path = "integration/search_index_test.rs"]
mod search_index_test;
#[path = "integration/session_gc_test.rs"]
mod session_gc_test;
#[path = "integration/session_graph_serde_test.rs"]
mod session_graph_serde_test;
#[path = "integration/swift_parser_test.rs"]
mod swift_parser_test;
#[path = "integration/ts_parser_test.rs"]
mod ts_parser_test;
#[path = "integration/valkey_cache_test.rs"]
mod valkey_cache_test;
#[path = "integration/workspace_cache_test.rs"]
mod workspace_cache_test;
