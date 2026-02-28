use std::path::Path;

use dk_core::{Error, RepoId, Symbol, SymbolId};
use tantivy::collector::TopDocs;
use tantivy::query::{BooleanQuery, Occur, QueryParser, TermQuery};
use tantivy::schema::*;
use tantivy::{Directory, Index, IndexReader, IndexWriter, ReloadPolicy, TantivyDocument};
use uuid::Uuid;

/// Full-text search index for symbols, backed by Tantivy.
///
/// Indexes symbol metadata across multiple text fields and supports
/// filtering by repository. The index is stored on disk at the path
/// provided to [`SearchIndex::open`].
pub struct SearchIndex {
    index: Index,
    reader: IndexReader,
    writer: IndexWriter,
    // Field handles kept for building queries and documents.
    f_symbol_id: Field,
    f_repo_id: Field,
    f_name: Field,
    f_qualified_name: Field,
    f_signature: Field,
    f_doc_comment: Field,
    f_file_path: Field,
    f_kind: Field,
}

impl SearchIndex {
    /// Open or create a Tantivy index at the given directory path.
    ///
    /// Defines the schema with the following fields:
    /// - `symbol_id` — stored string (UUID)
    /// - `repo_id` — indexed string (not tokenized) for filtering
    /// - `name` — tokenized text field
    /// - `qualified_name` — tokenized text field
    /// - `signature` — tokenized text field
    /// - `doc_comment` — tokenized text field
    /// - `file_path` — tokenized text field
    /// - `kind` — indexed string (not tokenized)
    pub fn open(path: &Path) -> dk_core::Result<Self> {
        let mut schema_builder = Schema::builder();

        let f_symbol_id = schema_builder.add_text_field("symbol_id", STRING | STORED);
        let f_repo_id = schema_builder.add_text_field("repo_id", STRING);
        let f_name = schema_builder.add_text_field("name", TEXT);
        let f_qualified_name = schema_builder.add_text_field("qualified_name", TEXT);
        let f_signature = schema_builder.add_text_field("signature", TEXT);
        let f_doc_comment = schema_builder.add_text_field("doc_comment", TEXT);
        let f_file_path = schema_builder.add_text_field("file_path", TEXT);
        let f_kind = schema_builder.add_text_field("kind", STRING);

        let schema = schema_builder.build();

        let dir: Box<dyn Directory> = if path.exists() && path.join("meta.json").exists() {
            Box::new(
                tantivy::directory::MmapDirectory::open(path)
                    .map_err(|e| Error::Internal(format!("Failed to open index directory: {e}")))?,
            )
        } else {
            std::fs::create_dir_all(path)?;
            Box::new(
                tantivy::directory::MmapDirectory::open(path)
                    .map_err(|e| Error::Internal(format!("Failed to open index directory: {e}")))?,
            )
        };

        let index = Index::open_or_create(dir, schema.clone())
            .map_err(|e| Error::Internal(format!("Failed to open or create index: {e}")))?;

        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()
            .map_err(|e| Error::Internal(format!("Failed to create index reader: {e}")))?;

        let writer = index
            .writer(50_000_000) // 50 MB memory budget
            .map_err(|e| Error::Internal(format!("Failed to create index writer: {e}")))?;

        Ok(Self {
            index,
            reader,
            writer,
            f_symbol_id,
            f_repo_id,
            f_name,
            f_qualified_name,
            f_signature,
            f_doc_comment,
            f_file_path,
            f_kind,
        })
    }

    /// Add a symbol document to the index.
    pub fn index_symbol(&mut self, repo_id: RepoId, sym: &Symbol) -> dk_core::Result<()> {
        let mut doc = TantivyDocument::new();
        doc.add_text(self.f_symbol_id, sym.id.to_string());
        doc.add_text(self.f_repo_id, repo_id.to_string());
        doc.add_text(self.f_name, &sym.name);
        doc.add_text(self.f_qualified_name, &sym.qualified_name);
        if let Some(ref sig) = sym.signature {
            doc.add_text(self.f_signature, sig);
        }
        if let Some(ref doc_comment) = sym.doc_comment {
            doc.add_text(self.f_doc_comment, doc_comment);
        }
        doc.add_text(self.f_file_path, sym.file_path.to_string_lossy().as_ref());
        doc.add_text(self.f_kind, sym.kind.to_string());

        self.writer
            .add_document(doc)
            .map_err(|e| Error::Internal(format!("Failed to add document: {e}")))?;

        Ok(())
    }

    /// Delete a document by `symbol_id`.
    pub fn remove_symbol(&mut self, symbol_id: SymbolId) -> dk_core::Result<()> {
        let term = tantivy::Term::from_field_text(self.f_symbol_id, &symbol_id.to_string());
        self.writer.delete_term(term);
        Ok(())
    }

    /// Commit the index writer, making all pending additions and deletions
    /// visible to subsequent searches.
    pub fn commit(&mut self) -> dk_core::Result<()> {
        self.writer
            .commit()
            .map_err(|e| Error::Internal(format!("Failed to commit index: {e}")))?;

        // Reload the reader so subsequent searches see the latest commit.
        self.reader
            .reload()
            .map_err(|e| Error::Internal(format!("Failed to reload reader: {e}")))?;

        Ok(())
    }

    /// Search across all text fields, filtered by `repo_id`.
    ///
    /// Returns up to `limit` matching [`SymbolId`]s, ranked by relevance.
    pub fn search(
        &self,
        repo_id: RepoId,
        query: &str,
        limit: usize,
    ) -> dk_core::Result<Vec<SymbolId>> {
        let searcher = self.reader.searcher();

        // Build a repo_id filter as a TermQuery.
        let repo_term =
            tantivy::Term::from_field_text(self.f_repo_id, &repo_id.to_string());
        let repo_query = TermQuery::new(repo_term, IndexRecordOption::Basic);

        // Build a full-text query across the text fields using QueryParser.
        let text_fields = vec![
            self.f_name,
            self.f_qualified_name,
            self.f_signature,
            self.f_doc_comment,
            self.f_file_path,
        ];
        let query_parser = QueryParser::for_index(&self.index, text_fields);
        let text_query = query_parser
            .parse_query(query)
            .map_err(|e| Error::Internal(format!("Failed to parse query: {e}")))?;

        // Combine: MUST match repo_id AND MUST match text query.
        let combined = BooleanQuery::new(vec![
            (Occur::Must, Box::new(repo_query)),
            (Occur::Must, text_query),
        ]);

        let top_docs = searcher
            .search(&combined, &TopDocs::with_limit(limit))
            .map_err(|e| Error::Internal(format!("Search failed: {e}")))?;

        let mut results = Vec::with_capacity(top_docs.len());
        for (_score, doc_address) in top_docs {
            let doc: TantivyDocument = searcher
                .doc(doc_address)
                .map_err(|e| Error::Internal(format!("Failed to retrieve doc: {e}")))?;

            if let Some(id_value) = doc.get_first(self.f_symbol_id) {
                if let Some(id_str) = id_value.as_str() {
                    let uuid = Uuid::parse_str(id_str).map_err(|e| {
                        Error::Internal(format!("Invalid UUID in index: {e}"))
                    })?;
                    results.push(uuid);
                }
            }
        }

        Ok(results)
    }
}
