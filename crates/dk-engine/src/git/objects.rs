use std::path::Path;

use dk_core::{Error, Result};

/// Provides read/write access to Git objects (blobs) and working tree files
/// through a borrowed reference to a [`super::GitRepository`].
pub struct GitObjects<'a> {
    repo: &'a super::GitRepository,
}

impl<'a> GitObjects<'a> {
    /// Create a new `GitObjects` handle tied to the given repository.
    pub fn new(repo: &'a super::GitRepository) -> Self {
        Self { repo }
    }

    /// Write a blob to the Git object store and return its OID as a hex string.
    pub fn write_blob(&self, data: &[u8]) -> Result<String> {
        let id = self.repo.inner().write_blob(data).map_err(|e| {
            Error::Git(format!("failed to write blob: {}", e))
        })?;
        Ok(id.to_hex().to_string())
    }

    /// Read a blob from the Git object store by its OID hex string.
    ///
    /// Returns an error if the OID is invalid, the object is not found,
    /// or the object is not a blob.
    pub fn read_blob(&self, oid_hex: &str) -> Result<Vec<u8>> {
        let oid = gix::ObjectId::from_hex(oid_hex.as_bytes()).map_err(|e| {
            Error::Git(format!("invalid OID '{}': {}", oid_hex, e))
        })?;

        let object = self.repo.inner().find_object(oid).map_err(|e| {
            Error::Git(format!("object not found '{}': {}", oid_hex, e))
        })?;

        if object.kind != gix::object::Kind::Blob {
            return Err(Error::Git(format!(
                "object '{}' is not a blob (found {:?})",
                oid_hex, object.kind
            )));
        }

        Ok(object.data.to_vec())
    }

    /// Read a file from the working tree, relative to the repository root.
    pub fn read_file(&self, file_path: &Path) -> Result<Vec<u8>> {
        let full_path = self.repo.path().join(file_path);
        std::fs::read(&full_path).map_err(|e| {
            Error::Git(format!("failed to read file {}: {}", full_path.display(), e))
        })
    }

    /// Write a file to the working tree, relative to the repository root.
    ///
    /// Creates parent directories as needed.
    pub fn write_file(&self, file_path: &Path, content: &[u8]) -> Result<()> {
        let full_path = self.repo.path().join(file_path);

        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                Error::Git(format!(
                    "failed to create parent dirs for {}: {}",
                    full_path.display(),
                    e
                ))
            })?;
        }

        std::fs::write(&full_path, content).map_err(|e| {
            Error::Git(format!("failed to write file {}: {}", full_path.display(), e))
        })
    }
}
