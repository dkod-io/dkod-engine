//! Vector similarity search abstraction.
//!
//! Provides a trait for embedding-based semantic search that can be
//! backed by Qdrant or any other vector database. Gracefully degrades
//! to no-op when Qdrant is not configured.

use async_trait::async_trait;
use uuid::Uuid;

/// A search result from vector similarity.
#[derive(Debug, Clone)]
pub struct VectorSearchResult {
    pub symbol_id: Uuid,
    pub score: f32,
}

/// Trait for vector search backends.
#[async_trait]
pub trait VectorSearch: Send + Sync + 'static {
    async fn index_embedding(
        &self,
        symbol_id: Uuid,
        repo_id: Uuid,
        embedding: Vec<f32>,
    ) -> anyhow::Result<()>;

    async fn search_similar(
        &self,
        repo_id: Uuid,
        query_embedding: Vec<f32>,
        limit: usize,
    ) -> anyhow::Result<Vec<VectorSearchResult>>;

    async fn delete_embedding(&self, symbol_id: Uuid) -> anyhow::Result<()>;
}

/// No-op vector search (used when Qdrant is not configured).
pub struct NoOpVectorSearch;

#[async_trait]
impl VectorSearch for NoOpVectorSearch {
    async fn index_embedding(
        &self,
        _symbol_id: Uuid,
        _repo_id: Uuid,
        _embedding: Vec<f32>,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    async fn search_similar(
        &self,
        _repo_id: Uuid,
        _query_embedding: Vec<f32>,
        _limit: usize,
    ) -> anyhow::Result<Vec<VectorSearchResult>> {
        Ok(vec![])
    }

    async fn delete_embedding(&self, _symbol_id: Uuid) -> anyhow::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn noop_search_returns_empty() {
        let search = NoOpVectorSearch;
        let results = search
            .search_similar(Uuid::new_v4(), vec![0.1, 0.2, 0.3], 10)
            .await
            .unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn noop_index_succeeds() {
        let search = NoOpVectorSearch;
        search
            .index_embedding(Uuid::new_v4(), Uuid::new_v4(), vec![0.1, 0.2])
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn noop_delete_succeeds() {
        let search = NoOpVectorSearch;
        search.delete_embedding(Uuid::new_v4()).await.unwrap();
    }
}
