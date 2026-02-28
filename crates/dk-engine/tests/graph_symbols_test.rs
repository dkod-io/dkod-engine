use std::path::PathBuf;

use dk_core::{Span, Symbol, SymbolKind, Visibility};
use dk_engine::graph::SymbolStore;
use sqlx::PgPool;
use uuid::Uuid;

async fn setup_pool() -> PgPool {
    let url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://localhost/dekode_test".to_string());

    let pool = PgPool::connect(&url)
        .await
        .expect("Failed to connect to test database");

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("Failed to run migrations");

    pool
}

fn make_symbol(name: &str, kind: SymbolKind, file: &str) -> Symbol {
    Symbol {
        id: Uuid::new_v4(),
        name: name.to_string(),
        qualified_name: format!("test_mod::{name}"),
        kind,
        visibility: Visibility::Public,
        file_path: PathBuf::from(file),
        span: Span {
            start_byte: 0,
            end_byte: 100,
        },
        signature: Some(format!("fn {name}()")),
        doc_comment: Some(format!("Doc for {name}")),
        parent: None,
        last_modified_by: None,
        last_modified_intent: None,
    }
}

/// Create a test repository row and return its id.
async fn create_test_repo(pool: &PgPool) -> Uuid {
    let repo_id = Uuid::new_v4();
    sqlx::query("INSERT INTO repositories (id, name, path) VALUES ($1, $2, $3)")
        .bind(repo_id)
        .bind(format!("test-repo-{repo_id}"))
        .bind("/tmp/test")
        .execute(pool)
        .await
        .expect("Failed to create test repo");
    repo_id
}

/// Clean up test repository (cascades to symbols).
async fn cleanup_repo(pool: &PgPool, repo_id: Uuid) {
    sqlx::query("DELETE FROM repositories WHERE id = $1")
        .bind(repo_id)
        .execute(pool)
        .await
        .expect("Failed to clean up test repo");
}

#[tokio::test]
async fn test_upsert_and_get_by_id() {
    let pool = setup_pool().await;
    let repo_id = create_test_repo(&pool).await;
    let store = SymbolStore::new(pool.clone());

    let sym = make_symbol("hello", SymbolKind::Function, "src/lib.rs");
    store.upsert_symbol(repo_id, &sym).await.unwrap();

    let fetched = store.get_by_id(sym.id).await.unwrap();
    assert!(fetched.is_some());
    let fetched = fetched.unwrap();
    assert_eq!(fetched.name, "hello");
    assert_eq!(fetched.qualified_name, "test_mod::hello");
    assert_eq!(fetched.kind, SymbolKind::Function);
    assert_eq!(fetched.visibility, Visibility::Public);
    assert_eq!(fetched.file_path, PathBuf::from("src/lib.rs"));
    assert_eq!(fetched.span.start_byte, 0);
    assert_eq!(fetched.span.end_byte, 100);
    assert_eq!(fetched.signature.as_deref(), Some("fn hello()"));

    cleanup_repo(&pool, repo_id).await;
}

#[tokio::test]
async fn test_upsert_is_idempotent() {
    let pool = setup_pool().await;
    let repo_id = create_test_repo(&pool).await;
    let store = SymbolStore::new(pool.clone());

    let sym = make_symbol("foo", SymbolKind::Struct, "src/main.rs");
    store.upsert_symbol(repo_id, &sym).await.unwrap();
    store.upsert_symbol(repo_id, &sym).await.unwrap();

    // Should still be exactly one symbol with that qualified_name
    let results = store.find_symbols(repo_id, "foo").await.unwrap();
    assert_eq!(results.len(), 1);

    cleanup_repo(&pool, repo_id).await;
}

#[tokio::test]
async fn test_upsert_updates_on_conflict() {
    let pool = setup_pool().await;
    let repo_id = create_test_repo(&pool).await;
    let store = SymbolStore::new(pool.clone());

    let mut sym = make_symbol("bar", SymbolKind::Function, "src/lib.rs");
    store.upsert_symbol(repo_id, &sym).await.unwrap();

    // Change the kind and re-upsert with a new id (same qualified_name)
    sym.id = Uuid::new_v4();
    sym.kind = SymbolKind::Struct;
    store.upsert_symbol(repo_id, &sym).await.unwrap();

    let results = store.find_symbols(repo_id, "bar").await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].kind, SymbolKind::Struct);
    // id should be updated to the new one
    assert_eq!(results[0].id, sym.id);

    cleanup_repo(&pool, repo_id).await;
}

#[tokio::test]
async fn test_find_symbols() {
    let pool = setup_pool().await;
    let repo_id = create_test_repo(&pool).await;
    let store = SymbolStore::new(pool.clone());

    let sym1 = make_symbol("process_data", SymbolKind::Function, "src/lib.rs");
    let sym2 = make_symbol("DataProcessor", SymbolKind::Struct, "src/lib.rs");
    let sym3 = make_symbol("unrelated", SymbolKind::Function, "src/lib.rs");

    store.upsert_symbol(repo_id, &sym1).await.unwrap();
    store.upsert_symbol(repo_id, &sym2).await.unwrap();
    store.upsert_symbol(repo_id, &sym3).await.unwrap();

    // Search for "data" should match both (case-insensitive)
    let results = store.find_symbols(repo_id, "data").await.unwrap();
    assert_eq!(results.len(), 2);

    // Search for "unrelated" should match one
    let results = store.find_symbols(repo_id, "unrelated").await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name, "unrelated");

    cleanup_repo(&pool, repo_id).await;
}

#[tokio::test]
async fn test_find_by_kind() {
    let pool = setup_pool().await;
    let repo_id = create_test_repo(&pool).await;
    let store = SymbolStore::new(pool.clone());

    let fn_sym = make_symbol("my_func", SymbolKind::Function, "src/lib.rs");
    let struct_sym = make_symbol("MyStruct", SymbolKind::Struct, "src/lib.rs");
    let enum_sym = make_symbol("MyEnum", SymbolKind::Enum, "src/lib.rs");

    store.upsert_symbol(repo_id, &fn_sym).await.unwrap();
    store.upsert_symbol(repo_id, &struct_sym).await.unwrap();
    store.upsert_symbol(repo_id, &enum_sym).await.unwrap();

    let functions = store.find_by_kind(repo_id, &SymbolKind::Function).await.unwrap();
    assert_eq!(functions.len(), 1);
    assert_eq!(functions[0].name, "my_func");

    let structs = store.find_by_kind(repo_id, &SymbolKind::Struct).await.unwrap();
    assert_eq!(structs.len(), 1);
    assert_eq!(structs[0].name, "MyStruct");

    cleanup_repo(&pool, repo_id).await;
}

#[tokio::test]
async fn test_find_by_file() {
    let pool = setup_pool().await;
    let repo_id = create_test_repo(&pool).await;
    let store = SymbolStore::new(pool.clone());

    let sym1 = make_symbol("alpha", SymbolKind::Function, "src/a.rs");
    let sym2 = make_symbol("beta", SymbolKind::Function, "src/b.rs");
    let sym3 = make_symbol("gamma", SymbolKind::Struct, "src/a.rs");

    store.upsert_symbol(repo_id, &sym1).await.unwrap();
    store.upsert_symbol(repo_id, &sym2).await.unwrap();
    store.upsert_symbol(repo_id, &sym3).await.unwrap();

    let file_a = store.find_by_file(repo_id, "src/a.rs").await.unwrap();
    assert_eq!(file_a.len(), 2);

    let file_b = store.find_by_file(repo_id, "src/b.rs").await.unwrap();
    assert_eq!(file_b.len(), 1);
    assert_eq!(file_b[0].name, "beta");

    cleanup_repo(&pool, repo_id).await;
}

#[tokio::test]
async fn test_delete_by_file() {
    let pool = setup_pool().await;
    let repo_id = create_test_repo(&pool).await;
    let store = SymbolStore::new(pool.clone());

    let sym1 = make_symbol("to_delete_1", SymbolKind::Function, "src/old.rs");
    let sym2 = make_symbol("to_delete_2", SymbolKind::Struct, "src/old.rs");
    let sym3 = make_symbol("keep_me", SymbolKind::Function, "src/keep.rs");

    store.upsert_symbol(repo_id, &sym1).await.unwrap();
    store.upsert_symbol(repo_id, &sym2).await.unwrap();
    store.upsert_symbol(repo_id, &sym3).await.unwrap();

    let deleted = store.delete_by_file(repo_id, "src/old.rs").await.unwrap();
    assert_eq!(deleted, 2);

    let remaining = store.count(repo_id).await.unwrap();
    assert_eq!(remaining, 1);

    // The kept symbol should still be there
    let kept = store.find_by_file(repo_id, "src/keep.rs").await.unwrap();
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].name, "keep_me");

    cleanup_repo(&pool, repo_id).await;
}

#[tokio::test]
async fn test_count() {
    let pool = setup_pool().await;
    let repo_id = create_test_repo(&pool).await;
    let store = SymbolStore::new(pool.clone());

    assert_eq!(store.count(repo_id).await.unwrap(), 0);

    let sym1 = make_symbol("one", SymbolKind::Function, "src/lib.rs");
    let sym2 = make_symbol("two", SymbolKind::Struct, "src/lib.rs");
    store.upsert_symbol(repo_id, &sym1).await.unwrap();
    store.upsert_symbol(repo_id, &sym2).await.unwrap();

    assert_eq!(store.count(repo_id).await.unwrap(), 2);

    cleanup_repo(&pool, repo_id).await;
}

#[tokio::test]
async fn test_get_by_id_not_found() {
    let pool = setup_pool().await;
    let store = SymbolStore::new(pool.clone());

    let result = store.get_by_id(Uuid::new_v4()).await.unwrap();
    assert!(result.is_none());
}
