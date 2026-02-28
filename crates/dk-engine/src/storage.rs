//! Object storage abstraction.

use async_trait::async_trait;

/// Trait for object storage backends.
#[async_trait]
pub trait ObjectStore: Send + Sync + 'static {
    async fn get(&self, key: &str) -> anyhow::Result<Vec<u8>>;
    async fn put(&self, key: &str, data: Vec<u8>) -> anyhow::Result<()>;
    async fn delete(&self, key: &str) -> anyhow::Result<()>;
    async fn list(&self, prefix: &str) -> anyhow::Result<Vec<String>>;
    async fn exists(&self, key: &str) -> anyhow::Result<bool>;
}

/// Local filesystem object store.
pub struct LocalStore {
    root: std::path::PathBuf,
}

impl LocalStore {
    pub fn new(root: std::path::PathBuf) -> Self {
        Self { root }
    }
}

#[async_trait]
impl ObjectStore for LocalStore {
    async fn get(&self, key: &str) -> anyhow::Result<Vec<u8>> {
        let path = self.root.join(key);
        Ok(tokio::fs::read(path).await?)
    }

    async fn put(&self, key: &str, data: Vec<u8>) -> anyhow::Result<()> {
        let path = self.root.join(key);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        Ok(tokio::fs::write(path, data).await?)
    }

    async fn delete(&self, key: &str) -> anyhow::Result<()> {
        let path = self.root.join(key);
        match tokio::fs::remove_file(path).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    async fn list(&self, prefix: &str) -> anyhow::Result<Vec<String>> {
        let dir = self.root.join(prefix);
        let mut entries = Vec::new();
        if dir.exists() {
            let mut read_dir = tokio::fs::read_dir(&dir).await?;
            while let Some(entry) = read_dir.next_entry().await? {
                if let Some(name) = entry.file_name().to_str() {
                    let key = if prefix.is_empty() {
                        name.to_string()
                    } else {
                        format!("{prefix}/{name}")
                    };
                    entries.push(key);
                }
            }
        }
        Ok(entries)
    }

    async fn exists(&self, key: &str) -> anyhow::Result<bool> {
        let path = self.root.join(key);
        Ok(path.exists())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn local_store_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = LocalStore::new(dir.path().to_path_buf());

        store.put("test/file.txt", b"hello".to_vec()).await.unwrap();
        assert!(store.exists("test/file.txt").await.unwrap());

        let data = store.get("test/file.txt").await.unwrap();
        assert_eq!(data, b"hello");

        let keys = store.list("test").await.unwrap();
        assert_eq!(keys, vec!["test/file.txt"]);

        store.delete("test/file.txt").await.unwrap();
        assert!(!store.exists("test/file.txt").await.unwrap());
    }

    #[tokio::test]
    async fn local_store_get_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let store = LocalStore::new(dir.path().to_path_buf());
        let result = store.get("nonexistent").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn local_store_delete_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let store = LocalStore::new(dir.path().to_path_buf());
        store.delete("nonexistent").await.unwrap();
    }
}
