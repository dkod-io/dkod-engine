//! Code-intelligence graph operations for dk-helix.
//!
//! This file is the hand-authored equivalent of what `helix compile` would
//! generate from `db/schema.hx` + `db/queries.hx`.  It registers HTTP and MCP
//! handlers via the `#[handler]` / `#[mcp_handler]` proc-macros so they are
//! picked up by `inventory` in main.rs.
//!
//! Schema (mirrors db/schema.hx):
//!   Nodes:  Repo, File, Symbol, Commit
//!   Edges:  RepoHasFile, Defines, Calls, Imports, HasCommit, TouchedBy

use bumpalo::Bump;
use helix_db::{
    helix_engine::{
        traversal_core::{
            config::{Config, GraphConfig},
            ops::{
                g::G,
                in_::in_::InAdapter,
                out::out::OutAdapter,
                source::{add_e::AddEAdapter, add_n::AddNAdapter, n_from_id::NFromIdAdapter},
            },
            traversal_value::TraversalValue,
        },
        types::GraphError,
    },
    helix_gateway::router::router::{HandlerInput, IoContFn},
    protocol::{format::Format, response::Response, value::Value},
    utils::{id::uuid_str, properties::ImmutablePropertiesMap},
};
use helix_macros::handler;
use sonic_rs::{Deserialize, Serialize};
use std::sync::Arc;

// ── Schema JSON ───────────────────────────────────────────────────────────────
// Embedded so HelixDB knows the type/index layout at startup.

const SCHEMA_JSON: &str = r#"{
  "schema": {
    "nodes": [
      {
        "name": "Repo",
        "properties": {
          "id":               "ID",
          "label":            "String",
          "name":             "String",
          "url":              "String",
          "default_branch":   "String",
          "primary_language": "String",
          "indexed_at":       "I64"
        }
      },
      {
        "name": "File",
        "properties": {
          "id":            "ID",
          "label":         "String",
          "path":          "String",
          "repo_id":       "String",
          "language":      "String",
          "size_bytes":    "I64",
          "last_modified": "I64",
          "checksum":      "String"
        }
      },
      {
        "name": "Symbol",
        "properties": {
          "id":             "ID",
          "label":          "String",
          "qualified_name": "String",
          "name":           "String",
          "kind":           "String",
          "signature":      "String",
          "doc":            "String",
          "start_line":     "I64",
          "end_line":       "I64",
          "is_public":      "Boolean"
        }
      },
      {
        "name": "Commit",
        "properties": {
          "id":            "ID",
          "label":         "String",
          "sha":           "String",
          "message":       "String",
          "author_name":   "String",
          "author_email":  "String",
          "authored_at":   "I64",
          "files_changed": "I64",
          "insertions":    "I64",
          "deletions":     "I64"
        }
      }
    ],
    "vectors": [],
    "edges": [
      { "name": "RepoHasFile", "from": "Repo",   "to": "File",   "properties": {} },
      { "name": "Defines",     "from": "File",   "to": "Symbol", "properties": {} },
      { "name": "Calls",       "from": "Symbol", "to": "Symbol",
        "properties": { "call_count": "I64", "call_sites": "String" } },
      { "name": "Imports",     "from": "File",   "to": "File",
        "properties": { "import_path": "String" } },
      { "name": "HasCommit",   "from": "Repo",   "to": "Commit", "properties": {} },
      { "name": "TouchedBy",   "from": "File",   "to": "Commit", "properties": {} }
    ]
  },
  "queries": [
    { "name": "UpsertRepo",            "parameters": { "name": "String", "url": "String", "default_branch": "String", "primary_language": "String", "indexed_at": "I64" }, "returns": ["repo"] },
    { "name": "AddFile",               "parameters": { "repo_id": "ID", "path": "String", "language": "String", "size_bytes": "I64", "last_modified": "I64", "checksum": "String" }, "returns": ["file"] },
    { "name": "AddSymbol",             "parameters": { "file_id": "ID", "qualified_name": "String", "name": "String", "kind": "String", "signature": "String", "doc": "String", "start_line": "I64", "end_line": "I64", "is_public": "Boolean" }, "returns": ["symbol"] },
    { "name": "AddCallEdge",           "parameters": { "caller_id": "ID", "callee_id": "ID", "call_count": "I64", "call_sites": "String" }, "returns": [] },
    { "name": "AddImportEdge",         "parameters": { "from_file_id": "ID", "to_file_id": "ID", "import_path": "String" }, "returns": [] },
    { "name": "AddCommit",             "parameters": { "repo_id": "ID", "sha": "String", "message": "String", "author_name": "String", "author_email": "String", "authored_at": "I64", "files_changed": "I64", "insertions": "I64", "deletions": "I64" }, "returns": ["commit"] },
    { "name": "GetRepoFiles",          "parameters": { "repo_id": "ID" }, "returns": ["files"] },
    { "name": "GetFileSymbols",        "parameters": { "file_id": "ID" }, "returns": ["symbols"] },
    { "name": "GetSymbolCallees",      "parameters": { "caller_id": "ID" }, "returns": ["callees"] },
    { "name": "GetSymbolCallers",      "parameters": { "callee_id": "ID" }, "returns": ["callers"] },
    { "name": "GetRepoCommits",        "parameters": { "repo_id": "ID" }, "returns": ["commits"] },
    { "name": "GetFileImports",        "parameters": { "file_id": "ID" }, "returns": ["imports"] },
    { "name": "GetFilesChangedInCommit","parameters": { "commit_id": "ID" }, "returns": ["files"] }
  ]
}"#;

pub fn config() -> Option<Config> {
    Some(Config {
        vector_config: None,
        graph_config: Some(GraphConfig {
            secondary_indices: Some(vec![
                "name".to_string(),             // Repo.name index
                "path".to_string(),             // File.path index
                "qualified_name".to_string(),   // Symbol.qualified_name index
                "sha".to_string(),              // Commit.sha index
            ]),
        }),
        db_max_size_gb: Some(4),
        mcp: Some(true),
        bm25: Some(false),
        schema: Some(SCHEMA_JSON.to_string()),
    })
}

// ── Input structs ─────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone)]
pub struct UpsertRepoInput {
    pub name:             String,
    pub url:              String,
    pub default_branch:   String,
    pub primary_language: String,
    pub indexed_at:       i64,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct AddFileInput {
    pub repo_id:       String,
    pub path:          String,
    pub language:      String,
    pub size_bytes:    i64,
    pub last_modified: i64,
    pub checksum:      String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct AddSymbolInput {
    pub file_id:        String,
    pub qualified_name: String,
    pub name:           String,
    pub kind:           String,
    pub signature:      String,
    pub doc:            String,
    pub start_line:     i64,
    pub end_line:       i64,
    pub is_public:      bool,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct AddCallEdgeInput {
    pub caller_id:  String,
    pub callee_id:  String,
    pub call_count: i64,
    pub call_sites: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct AddImportEdgeInput {
    pub from_file_id: String,
    pub to_file_id:   String,
    pub import_path:  String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct AddCommitInput {
    pub repo_id:       String,
    pub sha:           String,
    pub message:       String,
    pub author_name:   String,
    pub author_email:  String,
    pub authored_at:   i64,
    pub files_changed: i64,
    pub insertions:    i64,
    pub deletions:     i64,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct IdInput {
    pub id: String,
}

// ── Mutation handlers ─────────────────────────────────────────────────────────

#[handler]
pub fn UpsertRepo(input: HandlerInput) -> Result<Response, GraphError> {
    let db = Arc::clone(&input.graph.storage);
    let data = input.request.in_fmt.deserialize::<UpsertRepoInput>(&input.request.body)?;
    let arena = Bump::new();
    let mut txn = db.graph_env.write_txn()
        .map_err(|e| GraphError::New(format!("write_txn: {e:?}")))?;

    let repo = G::new_mut(&db, &arena, &mut txn)
        .add_n(
            "Repo",
            Some(ImmutablePropertiesMap::new(
                5,
                vec![
                    ("name",             Value::from(&data.name)),
                    ("url",              Value::from(&data.url)),
                    ("default_branch",   Value::from(&data.default_branch)),
                    ("primary_language", Value::from(&data.primary_language)),
                    ("indexed_at",       Value::from(data.indexed_at)),
                ]
                .into_iter(),
                &arena,
            )),
            Some(&["name"]),
        )
        .collect_to_obj()?;

    let repo_id = uuid_str(repo.id(), &arena);
    txn.commit().map_err(|e| GraphError::New(format!("commit: {e:?}")))?;

    Ok(input.request.out_fmt.create_response(&sonic_rs::json!({ "repo_id": repo_id })))
}

#[handler]
pub fn AddFile(input: HandlerInput) -> Result<Response, GraphError> {
    let db = Arc::clone(&input.graph.storage);
    let data = input.request.in_fmt.deserialize::<AddFileInput>(&input.request.body)?;
    let arena = Bump::new();
    let mut txn = db.graph_env.write_txn()
        .map_err(|e| GraphError::New(format!("write_txn: {e:?}")))?;

    let repo = G::new(&db, &txn, &arena)
        .n_from_id(&data.repo_id)
        .collect_to_obj()?;

    let file = G::new_mut(&db, &arena, &mut txn)
        .add_n(
            "File",
            Some(ImmutablePropertiesMap::new(
                6,
                vec![
                    ("path",          Value::from(&data.path)),
                    ("repo_id",       Value::from(&data.repo_id)),
                    ("language",      Value::from(&data.language)),
                    ("size_bytes",    Value::from(data.size_bytes)),
                    ("last_modified", Value::from(data.last_modified)),
                    ("checksum",      Value::from(&data.checksum)),
                ]
                .into_iter(),
                &arena,
            )),
            Some(&["path"]),
        )
        .collect_to_obj()?;

    G::new_mut(&db, &arena, &mut txn)
        .add_edge("RepoHasFile", None, repo.id(), file.id(), false)
        .collect_to_obj()?;

    let file_id = uuid_str(file.id(), &arena);
    txn.commit().map_err(|e| GraphError::New(format!("commit: {e:?}")))?;

    Ok(input.request.out_fmt.create_response(&sonic_rs::json!({ "file_id": file_id })))
}

#[handler]
pub fn AddSymbol(input: HandlerInput) -> Result<Response, GraphError> {
    let db = Arc::clone(&input.graph.storage);
    let data = input.request.in_fmt.deserialize::<AddSymbolInput>(&input.request.body)?;
    let arena = Bump::new();
    let mut txn = db.graph_env.write_txn()
        .map_err(|e| GraphError::New(format!("write_txn: {e:?}")))?;

    let file = G::new(&db, &txn, &arena)
        .n_from_id(&data.file_id)
        .collect_to_obj()?;

    let sym = G::new_mut(&db, &arena, &mut txn)
        .add_n(
            "Symbol",
            Some(ImmutablePropertiesMap::new(
                9,
                vec![
                    ("qualified_name", Value::from(&data.qualified_name)),
                    ("name",           Value::from(&data.name)),
                    ("kind",           Value::from(&data.kind)),
                    ("signature",      Value::from(&data.signature)),
                    ("doc",            Value::from(&data.doc)),
                    ("start_line",     Value::from(data.start_line)),
                    ("end_line",       Value::from(data.end_line)),
                    ("is_public",      Value::from(data.is_public)),
                ]
                .into_iter(),
                &arena,
            )),
            Some(&["qualified_name"]),
        )
        .collect_to_obj()?;

    G::new_mut(&db, &arena, &mut txn)
        .add_edge("Defines", None, file.id(), sym.id(), false)
        .collect_to_obj()?;

    let sym_id = uuid_str(sym.id(), &arena);
    txn.commit().map_err(|e| GraphError::New(format!("commit: {e:?}")))?;

    Ok(input.request.out_fmt.create_response(&sonic_rs::json!({ "symbol_id": sym_id })))
}

#[handler]
pub fn AddCallEdge(input: HandlerInput) -> Result<Response, GraphError> {
    let db = Arc::clone(&input.graph.storage);
    let data = input.request.in_fmt.deserialize::<AddCallEdgeInput>(&input.request.body)?;
    let arena = Bump::new();
    let mut txn = db.graph_env.write_txn()
        .map_err(|e| GraphError::New(format!("write_txn: {e:?}")))?;

    let caller = G::new(&db, &txn, &arena).n_from_id(&data.caller_id).collect_to_obj()?;
    let callee = G::new(&db, &txn, &arena).n_from_id(&data.callee_id).collect_to_obj()?;

    G::new_mut(&db, &arena, &mut txn)
        .add_edge(
            "Calls",
            Some(ImmutablePropertiesMap::new(
                2,
                vec![
                    ("call_count", Value::from(data.call_count)),
                    ("call_sites", Value::from(&data.call_sites)),
                ]
                .into_iter(),
                &arena,
            )),
            caller.id(),
            callee.id(),
            false,
        )
        .collect_to_obj()?;

    txn.commit().map_err(|e| GraphError::New(format!("commit: {e:?}")))?;
    Ok(input.request.out_fmt.create_response(&sonic_rs::json!({ "ok": true })))
}

#[handler]
pub fn AddImportEdge(input: HandlerInput) -> Result<Response, GraphError> {
    let db = Arc::clone(&input.graph.storage);
    let data = input.request.in_fmt.deserialize::<AddImportEdgeInput>(&input.request.body)?;
    let arena = Bump::new();
    let mut txn = db.graph_env.write_txn()
        .map_err(|e| GraphError::New(format!("write_txn: {e:?}")))?;

    let from_f = G::new(&db, &txn, &arena).n_from_id(&data.from_file_id).collect_to_obj()?;
    let to_f   = G::new(&db, &txn, &arena).n_from_id(&data.to_file_id).collect_to_obj()?;

    G::new_mut(&db, &arena, &mut txn)
        .add_edge(
            "Imports",
            Some(ImmutablePropertiesMap::new(
                1,
                vec![("import_path", Value::from(&data.import_path))].into_iter(),
                &arena,
            )),
            from_f.id(),
            to_f.id(),
            false,
        )
        .collect_to_obj()?;

    txn.commit().map_err(|e| GraphError::New(format!("commit: {e:?}")))?;
    Ok(input.request.out_fmt.create_response(&sonic_rs::json!({ "ok": true })))
}

#[handler]
pub fn AddCommit(input: HandlerInput) -> Result<Response, GraphError> {
    let db = Arc::clone(&input.graph.storage);
    let data = input.request.in_fmt.deserialize::<AddCommitInput>(&input.request.body)?;
    let arena = Bump::new();
    let mut txn = db.graph_env.write_txn()
        .map_err(|e| GraphError::New(format!("write_txn: {e:?}")))?;

    let repo = G::new(&db, &txn, &arena).n_from_id(&data.repo_id).collect_to_obj()?;

    let commit = G::new_mut(&db, &arena, &mut txn)
        .add_n(
            "Commit",
            Some(ImmutablePropertiesMap::new(
                9,
                vec![
                    ("sha",           Value::from(&data.sha)),
                    ("message",       Value::from(&data.message)),
                    ("author_name",   Value::from(&data.author_name)),
                    ("author_email",  Value::from(&data.author_email)),
                    ("authored_at",   Value::from(data.authored_at)),
                    ("files_changed", Value::from(data.files_changed)),
                    ("insertions",    Value::from(data.insertions)),
                    ("deletions",     Value::from(data.deletions)),
                ]
                .into_iter(),
                &arena,
            )),
            Some(&["sha"]),
        )
        .collect_to_obj()?;

    G::new_mut(&db, &arena, &mut txn)
        .add_edge("HasCommit", None, repo.id(), commit.id(), false)
        .collect_to_obj()?;

    let commit_id = uuid_str(commit.id(), &arena);
    txn.commit().map_err(|e| GraphError::New(format!("commit: {e:?}")))?;

    Ok(input.request.out_fmt.create_response(&sonic_rs::json!({ "commit_id": commit_id })))
}

// ── Read handlers ─────────────────────────────────────────────────────────────

#[handler]
pub fn GetRepoFiles(input: HandlerInput) -> Result<Response, GraphError> {
    let db = Arc::clone(&input.graph.storage);
    let data = input.request.in_fmt.deserialize::<IdInput>(&input.request.body)?;
    let arena = Bump::new();
    let txn = db.graph_env.read_txn()
        .map_err(|e| GraphError::New(format!("read_txn: {e:?}")))?;

    let files = G::new(&db, &txn, &arena)
        .n_from_id(&data.id)
        .out("RepoHasFile")
        .collect_to_obj()?;

    Ok(input.request.out_fmt.create_response(&files))
}

#[handler]
pub fn GetFileSymbols(input: HandlerInput) -> Result<Response, GraphError> {
    let db = Arc::clone(&input.graph.storage);
    let data = input.request.in_fmt.deserialize::<IdInput>(&input.request.body)?;
    let arena = Bump::new();
    let txn = db.graph_env.read_txn()
        .map_err(|e| GraphError::New(format!("read_txn: {e:?}")))?;

    let symbols = G::new(&db, &txn, &arena)
        .n_from_id(&data.id)
        .out("Defines")
        .collect_to_obj()?;

    Ok(input.request.out_fmt.create_response(&symbols))
}

#[handler]
pub fn GetSymbolCallees(input: HandlerInput) -> Result<Response, GraphError> {
    let db = Arc::clone(&input.graph.storage);
    let data = input.request.in_fmt.deserialize::<IdInput>(&input.request.body)?;
    let arena = Bump::new();
    let txn = db.graph_env.read_txn()
        .map_err(|e| GraphError::New(format!("read_txn: {e:?}")))?;

    let callees = G::new(&db, &txn, &arena)
        .n_from_id(&data.id)
        .out("Calls")
        .collect_to_obj()?;

    Ok(input.request.out_fmt.create_response(&callees))
}

#[handler]
pub fn GetSymbolCallers(input: HandlerInput) -> Result<Response, GraphError> {
    let db = Arc::clone(&input.graph.storage);
    let data = input.request.in_fmt.deserialize::<IdInput>(&input.request.body)?;
    let arena = Bump::new();
    let txn = db.graph_env.read_txn()
        .map_err(|e| GraphError::New(format!("read_txn: {e:?}")))?;

    let callers = G::new(&db, &txn, &arena)
        .n_from_id(&data.id)
        .in_("Calls")
        .collect_to_obj()?;

    Ok(input.request.out_fmt.create_response(&callers))
}

#[handler]
pub fn GetRepoCommits(input: HandlerInput) -> Result<Response, GraphError> {
    let db = Arc::clone(&input.graph.storage);
    let data = input.request.in_fmt.deserialize::<IdInput>(&input.request.body)?;
    let arena = Bump::new();
    let txn = db.graph_env.read_txn()
        .map_err(|e| GraphError::New(format!("read_txn: {e:?}")))?;

    let commits = G::new(&db, &txn, &arena)
        .n_from_id(&data.id)
        .out("HasCommit")
        .collect_to_obj()?;

    Ok(input.request.out_fmt.create_response(&commits))
}

#[handler]
pub fn GetFileImports(input: HandlerInput) -> Result<Response, GraphError> {
    let db = Arc::clone(&input.graph.storage);
    let data = input.request.in_fmt.deserialize::<IdInput>(&input.request.body)?;
    let arena = Bump::new();
    let txn = db.graph_env.read_txn()
        .map_err(|e| GraphError::New(format!("read_txn: {e:?}")))?;

    let imports = G::new(&db, &txn, &arena)
        .n_from_id(&data.id)
        .out("Imports")
        .collect_to_obj()?;

    Ok(input.request.out_fmt.create_response(&imports))
}

#[handler]
pub fn GetFilesChangedInCommit(input: HandlerInput) -> Result<Response, GraphError> {
    let db = Arc::clone(&input.graph.storage);
    let data = input.request.in_fmt.deserialize::<IdInput>(&input.request.body)?;
    let arena = Bump::new();
    let txn = db.graph_env.read_txn()
        .map_err(|e| GraphError::New(format!("read_txn: {e:?}")))?;

    let files = G::new(&db, &txn, &arena)
        .n_from_id(&data.id)
        .in_("TouchedBy")
        .collect_to_obj()?;

    Ok(input.request.out_fmt.create_response(&files))
}
