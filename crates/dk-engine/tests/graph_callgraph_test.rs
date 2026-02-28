use std::path::PathBuf;

use dk_core::{
    CallEdge, CallKind, Dependency, Span, Symbol, SymbolKind, TypeInfo, Visibility,
};
use dk_engine::graph::{CallGraphStore, DependencyStore, SymbolStore, TypeInfoStore};
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
        doc_comment: None,
        parent: None,
        last_modified_by: None,
        last_modified_intent: None,
    }
}

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

async fn cleanup_repo(pool: &PgPool, repo_id: Uuid) {
    sqlx::query("DELETE FROM repositories WHERE id = $1")
        .bind(repo_id)
        .execute(pool)
        .await
        .expect("Failed to clean up test repo");
}

// ── CallGraphStore tests ──

#[tokio::test]
async fn test_insert_edge_and_find_callees() {
    let pool = setup_pool().await;
    let repo_id = create_test_repo(&pool).await;
    let sym_store = SymbolStore::new(pool.clone());
    let cg_store = CallGraphStore::new(pool.clone());

    let caller = make_symbol("main", SymbolKind::Function, "src/main.rs");
    let callee = make_symbol("helper", SymbolKind::Function, "src/lib.rs");
    sym_store.upsert_symbol(repo_id, &caller).await.unwrap();
    sym_store.upsert_symbol(repo_id, &callee).await.unwrap();

    let edge = CallEdge {
        id: Uuid::new_v4(),
        repo_id,
        caller: caller.id,
        callee: callee.id,
        kind: CallKind::DirectCall,
    };
    cg_store.insert_edge(&edge).await.unwrap();

    let callees = cg_store.find_callees(caller.id).await.unwrap();
    assert_eq!(callees.len(), 1);
    assert_eq!(callees[0].callee, callee.id);
    assert_eq!(callees[0].kind, CallKind::DirectCall);

    cleanup_repo(&pool, repo_id).await;
}

#[tokio::test]
async fn test_find_callers() {
    let pool = setup_pool().await;
    let repo_id = create_test_repo(&pool).await;
    let sym_store = SymbolStore::new(pool.clone());
    let cg_store = CallGraphStore::new(pool.clone());

    let caller_a = make_symbol("func_a", SymbolKind::Function, "src/a.rs");
    let caller_b = make_symbol("func_b", SymbolKind::Function, "src/b.rs");
    let target = make_symbol("target", SymbolKind::Function, "src/lib.rs");

    sym_store.upsert_symbol(repo_id, &caller_a).await.unwrap();
    sym_store.upsert_symbol(repo_id, &caller_b).await.unwrap();
    sym_store.upsert_symbol(repo_id, &target).await.unwrap();

    let edge_a = CallEdge {
        id: Uuid::new_v4(),
        repo_id,
        caller: caller_a.id,
        callee: target.id,
        kind: CallKind::DirectCall,
    };
    let edge_b = CallEdge {
        id: Uuid::new_v4(),
        repo_id,
        caller: caller_b.id,
        callee: target.id,
        kind: CallKind::MethodCall,
    };
    cg_store.insert_edge(&edge_a).await.unwrap();
    cg_store.insert_edge(&edge_b).await.unwrap();

    let callers = cg_store.find_callers(target.id).await.unwrap();
    assert_eq!(callers.len(), 2);

    cleanup_repo(&pool, repo_id).await;
}

#[tokio::test]
async fn test_insert_edge_idempotent() {
    let pool = setup_pool().await;
    let repo_id = create_test_repo(&pool).await;
    let sym_store = SymbolStore::new(pool.clone());
    let cg_store = CallGraphStore::new(pool.clone());

    let caller = make_symbol("caller_idem", SymbolKind::Function, "src/main.rs");
    let callee = make_symbol("callee_idem", SymbolKind::Function, "src/lib.rs");
    sym_store.upsert_symbol(repo_id, &caller).await.unwrap();
    sym_store.upsert_symbol(repo_id, &callee).await.unwrap();

    let edge = CallEdge {
        id: Uuid::new_v4(),
        repo_id,
        caller: caller.id,
        callee: callee.id,
        kind: CallKind::Import,
    };
    cg_store.insert_edge(&edge).await.unwrap();
    // Insert again — should not fail
    cg_store.insert_edge(&edge).await.unwrap();

    let callees = cg_store.find_callees(caller.id).await.unwrap();
    assert_eq!(callees.len(), 1);

    cleanup_repo(&pool, repo_id).await;
}

#[tokio::test]
async fn test_delete_edges_for_file() {
    let pool = setup_pool().await;
    let repo_id = create_test_repo(&pool).await;
    let sym_store = SymbolStore::new(pool.clone());
    let cg_store = CallGraphStore::new(pool.clone());

    let caller_in_file = make_symbol("in_file", SymbolKind::Function, "src/old.rs");
    let caller_other = make_symbol("other_file", SymbolKind::Function, "src/keep.rs");
    let callee = make_symbol("target_del", SymbolKind::Function, "src/lib.rs");

    sym_store
        .upsert_symbol(repo_id, &caller_in_file)
        .await
        .unwrap();
    sym_store
        .upsert_symbol(repo_id, &caller_other)
        .await
        .unwrap();
    sym_store.upsert_symbol(repo_id, &callee).await.unwrap();

    let edge1 = CallEdge {
        id: Uuid::new_v4(),
        repo_id,
        caller: caller_in_file.id,
        callee: callee.id,
        kind: CallKind::DirectCall,
    };
    let edge2 = CallEdge {
        id: Uuid::new_v4(),
        repo_id,
        caller: caller_other.id,
        callee: callee.id,
        kind: CallKind::DirectCall,
    };
    cg_store.insert_edge(&edge1).await.unwrap();
    cg_store.insert_edge(&edge2).await.unwrap();

    let deleted = cg_store
        .delete_edges_for_file(repo_id, "src/old.rs")
        .await
        .unwrap();
    assert_eq!(deleted, 1);

    // The edge from the other file should still exist
    let callers = cg_store.find_callers(callee.id).await.unwrap();
    assert_eq!(callers.len(), 1);
    assert_eq!(callers[0].caller, caller_other.id);

    cleanup_repo(&pool, repo_id).await;
}

// ── DependencyStore tests ──

#[tokio::test]
async fn test_upsert_and_find_dependencies() {
    let pool = setup_pool().await;
    let repo_id = create_test_repo(&pool).await;
    let dep_store = DependencyStore::new(pool.clone());

    let dep = Dependency {
        id: Uuid::new_v4(),
        repo_id,
        package: "serde".to_string(),
        version_req: "^1.0".to_string(),
    };
    dep_store.upsert_dependency(&dep).await.unwrap();

    let deps = dep_store.find_by_repo(repo_id).await.unwrap();
    assert_eq!(deps.len(), 1);
    assert_eq!(deps[0].package, "serde");
    assert_eq!(deps[0].version_req, "^1.0");

    cleanup_repo(&pool, repo_id).await;
}

#[tokio::test]
async fn test_upsert_dependency_updates_version() {
    let pool = setup_pool().await;
    let repo_id = create_test_repo(&pool).await;
    let dep_store = DependencyStore::new(pool.clone());

    let dep = Dependency {
        id: Uuid::new_v4(),
        repo_id,
        package: "tokio".to_string(),
        version_req: "^1.0".to_string(),
    };
    dep_store.upsert_dependency(&dep).await.unwrap();

    // Update version_req
    let dep2 = Dependency {
        id: Uuid::new_v4(),
        repo_id,
        package: "tokio".to_string(),
        version_req: "^1.38".to_string(),
    };
    dep_store.upsert_dependency(&dep2).await.unwrap();

    let deps = dep_store.find_by_repo(repo_id).await.unwrap();
    assert_eq!(deps.len(), 1);
    assert_eq!(deps[0].version_req, "^1.38");

    cleanup_repo(&pool, repo_id).await;
}

#[tokio::test]
async fn test_link_symbol_to_dep() {
    let pool = setup_pool().await;
    let repo_id = create_test_repo(&pool).await;
    let sym_store = SymbolStore::new(pool.clone());
    let dep_store = DependencyStore::new(pool.clone());

    let sym = make_symbol("use_serde", SymbolKind::Function, "src/lib.rs");
    sym_store.upsert_symbol(repo_id, &sym).await.unwrap();

    let dep = Dependency {
        id: Uuid::new_v4(),
        repo_id,
        package: "serde".to_string(),
        version_req: "^1.0".to_string(),
    };
    dep_store.upsert_dependency(&dep).await.unwrap();

    // Link should succeed
    dep_store
        .link_symbol_to_dep(sym.id, dep.id)
        .await
        .unwrap();

    // Linking again should be idempotent (ON CONFLICT DO NOTHING)
    dep_store
        .link_symbol_to_dep(sym.id, dep.id)
        .await
        .unwrap();

    cleanup_repo(&pool, repo_id).await;
}

#[tokio::test]
async fn test_delete_dependencies_by_repo() {
    let pool = setup_pool().await;
    let repo_id = create_test_repo(&pool).await;
    let dep_store = DependencyStore::new(pool.clone());

    let dep1 = Dependency {
        id: Uuid::new_v4(),
        repo_id,
        package: "anyhow".to_string(),
        version_req: "^1".to_string(),
    };
    let dep2 = Dependency {
        id: Uuid::new_v4(),
        repo_id,
        package: "thiserror".to_string(),
        version_req: "^2".to_string(),
    };
    dep_store.upsert_dependency(&dep1).await.unwrap();
    dep_store.upsert_dependency(&dep2).await.unwrap();

    let deleted = dep_store.delete_by_repo(repo_id).await.unwrap();
    assert_eq!(deleted, 2);

    let deps = dep_store.find_by_repo(repo_id).await.unwrap();
    assert!(deps.is_empty());

    cleanup_repo(&pool, repo_id).await;
}

// ── TypeInfoStore tests ──

#[tokio::test]
async fn test_upsert_and_get_type_info() {
    let pool = setup_pool().await;
    let repo_id = create_test_repo(&pool).await;
    let sym_store = SymbolStore::new(pool.clone());
    let ti_store = TypeInfoStore::new(pool.clone());

    let sym = make_symbol("MyStruct", SymbolKind::Struct, "src/lib.rs");
    sym_store.upsert_symbol(repo_id, &sym).await.unwrap();

    let info = TypeInfo {
        symbol_id: sym.id,
        params: vec![
            ("T".to_string(), "Clone".to_string()),
            ("U".to_string(), "Debug".to_string()),
        ],
        return_type: None,
        fields: vec![
            ("name".to_string(), "String".to_string()),
            ("age".to_string(), "u32".to_string()),
        ],
        implements: vec!["Clone".to_string(), "Debug".to_string()],
    };
    ti_store.upsert_type_info(&info).await.unwrap();

    let fetched = ti_store.get_by_symbol_id(sym.id).await.unwrap();
    assert!(fetched.is_some());
    let fetched = fetched.unwrap();
    assert_eq!(fetched.symbol_id, sym.id);
    assert_eq!(fetched.params.len(), 2);
    assert_eq!(fetched.params[0], ("T".to_string(), "Clone".to_string()));
    assert_eq!(fetched.params[1], ("U".to_string(), "Debug".to_string()));
    assert!(fetched.return_type.is_none());
    assert_eq!(fetched.fields.len(), 2);
    assert_eq!(fetched.fields[0], ("name".to_string(), "String".to_string()));
    assert_eq!(fetched.implements, vec!["Clone", "Debug"]);

    cleanup_repo(&pool, repo_id).await;
}

#[tokio::test]
async fn test_upsert_type_info_updates() {
    let pool = setup_pool().await;
    let repo_id = create_test_repo(&pool).await;
    let sym_store = SymbolStore::new(pool.clone());
    let ti_store = TypeInfoStore::new(pool.clone());

    let sym = make_symbol("my_func_ti", SymbolKind::Function, "src/lib.rs");
    sym_store.upsert_symbol(repo_id, &sym).await.unwrap();

    let info1 = TypeInfo {
        symbol_id: sym.id,
        params: vec![("x".to_string(), "i32".to_string())],
        return_type: Some("bool".to_string()),
        fields: vec![],
        implements: vec![],
    };
    ti_store.upsert_type_info(&info1).await.unwrap();

    // Update with new return type
    let info2 = TypeInfo {
        symbol_id: sym.id,
        params: vec![
            ("x".to_string(), "i32".to_string()),
            ("y".to_string(), "i32".to_string()),
        ],
        return_type: Some("i64".to_string()),
        fields: vec![],
        implements: vec![],
    };
    ti_store.upsert_type_info(&info2).await.unwrap();

    let fetched = ti_store.get_by_symbol_id(sym.id).await.unwrap().unwrap();
    assert_eq!(fetched.params.len(), 2);
    assert_eq!(fetched.return_type.as_deref(), Some("i64"));

    cleanup_repo(&pool, repo_id).await;
}

#[tokio::test]
async fn test_delete_type_info() {
    let pool = setup_pool().await;
    let repo_id = create_test_repo(&pool).await;
    let sym_store = SymbolStore::new(pool.clone());
    let ti_store = TypeInfoStore::new(pool.clone());

    let sym = make_symbol("to_delete_ti", SymbolKind::Struct, "src/lib.rs");
    sym_store.upsert_symbol(repo_id, &sym).await.unwrap();

    let info = TypeInfo {
        symbol_id: sym.id,
        params: vec![],
        return_type: None,
        fields: vec![("x".to_string(), "f64".to_string())],
        implements: vec!["Display".to_string()],
    };
    ti_store.upsert_type_info(&info).await.unwrap();

    let deleted = ti_store.delete_by_symbol_id(sym.id).await.unwrap();
    assert_eq!(deleted, 1);

    let fetched = ti_store.get_by_symbol_id(sym.id).await.unwrap();
    assert!(fetched.is_none());

    cleanup_repo(&pool, repo_id).await;
}

#[tokio::test]
async fn test_get_type_info_not_found() {
    let pool = setup_pool().await;
    let ti_store = TypeInfoStore::new(pool.clone());

    let result = ti_store.get_by_symbol_id(Uuid::new_v4()).await.unwrap();
    assert!(result.is_none());
}
