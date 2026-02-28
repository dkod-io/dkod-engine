use std::path::Path;

use dk_core::{Error, Result};

/// A wrapper around `gix::Repository` providing a simplified interface
/// for Git repository operations.
pub struct GitRepository {
    inner: gix::Repository,
}

impl GitRepository {
    /// Initialize a new Git repository at the given path.
    ///
    /// Creates the directory (and parents) if it does not exist, then
    /// initializes a standard (non-bare) Git repository with a `.git` directory.
    pub fn init(path: &Path) -> Result<Self> {
        std::fs::create_dir_all(path).map_err(|e| {
            Error::Git(format!("failed to create directory {}: {}", path.display(), e))
        })?;

        let repo = gix::init(path).map_err(|e| {
            Error::Git(format!("failed to init repository at {}: {}", path.display(), e))
        })?;

        Ok(Self { inner: repo })
    }

    /// Open an existing Git repository at the given path.
    pub fn open(path: &Path) -> Result<Self> {
        let repo = gix::open(path).map_err(|e| {
            Error::Git(format!("failed to open repository at {}: {}", path.display(), e))
        })?;

        Ok(Self { inner: repo })
    }

    /// Get the working directory path of the repository.
    ///
    /// Returns the working tree directory if available, otherwise falls back
    /// to the `.git` directory itself.
    pub fn path(&self) -> &Path {
        self.inner
            .workdir()
            .unwrap_or_else(|| self.inner.git_dir())
    }

    /// Get a reference to the inner `gix::Repository`.
    pub fn inner(&self) -> &gix::Repository {
        &self.inner
    }

    /// Create a Git commit with the current working directory state.
    /// Uses command-line git for simplicity (gix's commit API is complex).
    pub fn commit(&self, message: &str, author_name: &str, author_email: &str) -> Result<String> {
        let workdir = self.path();

        // Stage all files
        let output = std::process::Command::new("git")
            .args(["add", "-A"])
            .current_dir(workdir)
            .output()
            .map_err(|e| Error::Git(format!("git add failed: {e}")))?;

        if !output.status.success() {
            return Err(Error::Git(format!(
                "git add failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        let output = std::process::Command::new("git")
            .args([
                "commit",
                "--allow-empty",
                "-m", message,
                "--author", &format!("{} <{}>", author_name, author_email),
            ])
            .current_dir(workdir)
            .output()
            .map_err(|e| Error::Git(format!("git commit failed: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.contains("nothing to commit") {
                return self.head_hash()?
                    .ok_or_else(|| Error::Git("no HEAD after commit".into()));
            }
            return Err(Error::Git(format!("git commit failed: {stderr}")));
        }

        self.head_hash()?
            .ok_or_else(|| Error::Git("no HEAD after commit".into()))
    }

    // ── NSI: Tree-based read/write operations ──────────────────────────

    /// Read a file's content from a specific commit's tree (NOT the working directory).
    /// This is the core isolation primitive for Native Session Isolation.
    ///
    /// `commit_hex` — hex SHA of the commit whose tree to read from.
    /// `path` — relative file path within the tree (e.g. "src/main.rs").
    ///
    /// Returns the raw bytes of the blob, or an error if the commit / path
    /// does not exist or the entry is not a blob.
    pub fn read_tree_entry(&self, commit_hex: &str, path: &str) -> Result<Vec<u8>> {
        let oid = gix::ObjectId::from_hex(commit_hex.as_bytes())
            .map_err(|e| Error::Git(format!("invalid commit hex '{commit_hex}': {e}")))?;

        let commit = self
            .inner
            .find_commit(oid)
            .map_err(|e| Error::Git(format!("failed to find commit {commit_hex}: {e}")))?;

        let tree = self
            .inner
            .find_tree(commit.tree_id().expect("commit always has tree"))
            .map_err(|e| Error::Git(format!("failed to find tree for commit {commit_hex}: {e}")))?;

        let entry = tree
            .lookup_entry_by_path(path)
            .map_err(|e| Error::Git(format!("failed to lookup '{path}' in {commit_hex}: {e}")))?
            .ok_or_else(|| Error::Git(format!("path '{path}' not found in commit {commit_hex}")))?;

        let object = entry
            .object()
            .map_err(|e| Error::Git(format!("failed to read object for '{path}': {e}")))?;

        if object.kind != gix::object::Kind::Blob {
            return Err(Error::Git(format!(
                "path '{path}' in commit {commit_hex} is not a blob (is {:?})",
                object.kind
            )));
        }

        Ok(object.data.clone())
    }

    /// List all files (recursive) in a commit's tree. Returns relative paths
    /// using forward-slash separators.
    ///
    /// Only non-tree entries (blobs, symlinks, submodules) are included.
    pub fn list_tree_files(&self, commit_hex: &str) -> Result<Vec<String>> {
        let oid = gix::ObjectId::from_hex(commit_hex.as_bytes())
            .map_err(|e| Error::Git(format!("invalid commit hex '{commit_hex}': {e}")))?;

        let commit = self
            .inner
            .find_commit(oid)
            .map_err(|e| Error::Git(format!("failed to find commit {commit_hex}: {e}")))?;

        let tree = self
            .inner
            .find_tree(commit.tree_id().expect("commit always has tree"))
            .map_err(|e| Error::Git(format!("failed to find tree for commit {commit_hex}: {e}")))?;

        let entries = tree
            .traverse()
            .breadthfirst
            .files()
            .map_err(|e| Error::Git(format!("tree traversal failed for {commit_hex}: {e}")))?;

        let paths = entries
            .into_iter()
            .filter(|e| !e.mode.is_tree())
            .map(|e| e.filepath.to_string())
            .collect();

        Ok(paths)
    }

    /// Build a new git tree by applying overlay changes on a base tree, create
    /// a commit, and update the working directory.
    ///
    /// For each overlay entry:
    /// - `Some(content)` → upsert a blob at that path
    /// - `None` → delete the entry at that path
    ///
    /// After committing, the working directory is updated via `git checkout HEAD -- .`.
    ///
    /// Returns the hex SHA of the new commit.
    pub fn commit_tree_overlay(
        &self,
        base_commit_hex: &str,
        overlay: &[(String, Option<Vec<u8>>)],
        parent_commit_hex: &str,
        message: &str,
        author_name: &str,
        author_email: &str,
    ) -> Result<String> {
        use gix::object::tree::EntryKind;

        // Parse the base commit to get its tree
        let base_oid = gix::ObjectId::from_hex(base_commit_hex.as_bytes())
            .map_err(|e| Error::Git(format!("invalid base commit hex '{base_commit_hex}': {e}")))?;

        let base_commit = self
            .inner
            .find_commit(base_oid)
            .map_err(|e| Error::Git(format!("failed to find base commit {base_commit_hex}: {e}")))?;

        let base_tree = self
            .inner
            .find_tree(base_commit.tree_id().expect("commit always has tree"))
            .map_err(|e| Error::Git(format!("failed to find base tree: {e}")))?;

        // Parse the parent commit
        let parent_oid = gix::ObjectId::from_hex(parent_commit_hex.as_bytes())
            .map_err(|e| Error::Git(format!("invalid parent commit hex '{parent_commit_hex}': {e}")))?;

        // Create a tree editor from the base tree
        let mut editor = self
            .inner
            .edit_tree(base_tree.id)
            .map_err(|e| Error::Git(format!("failed to create tree editor: {e}")))?;

        // Apply each overlay entry
        for (path, maybe_content) in overlay {
            match maybe_content {
                Some(content) => {
                    // Write the blob to the object database
                    let blob_id = self
                        .inner
                        .write_blob(content)
                        .map_err(|e| Error::Git(format!("failed to write blob for '{path}': {e}")))?;

                    // Upsert into the tree — detect executable by file extension heuristic
                    // (default to regular blob)
                    editor
                        .upsert(path.as_str(), EntryKind::Blob, blob_id.detach())
                        .map_err(|e| Error::Git(format!("failed to upsert '{path}': {e}")))?;
                }
                None => {
                    // Remove the entry from the tree
                    editor
                        .remove(path.as_str())
                        .map_err(|e| Error::Git(format!("failed to remove '{path}': {e}")))?;
                }
            }
        }

        // Write the modified tree to the object database
        let new_tree_id = editor
            .write()
            .map_err(|e| Error::Git(format!("failed to write edited tree: {e}")))?;

        // Build the commit object with explicit author/committer
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        let time = gix::date::Time {
            seconds: now_secs,
            offset: 0,
        };

        let sig = gix::actor::Signature {
            name: author_name.into(),
            email: author_email.into(),
            time,
        };

        let mut time_buf = gix::date::parse::TimeBuf::default();
        let sig_ref = sig.to_ref(&mut time_buf);

        let commit_id = self
            .inner
            .commit_as(sig_ref, sig_ref, "HEAD", message, new_tree_id.detach(), [parent_oid])
            .map_err(|e| Error::Git(format!("failed to create commit: {e}")))?;

        let commit_hex = commit_id.to_hex().to_string();

        // Update the working directory to match the new commit.
        // Spawn on a separate thread to avoid blocking the tokio async runtime
        // when this function is called from an async context via spawn_blocking.
        let work_dir = self.path().to_path_buf();
        let output = std::thread::spawn(move || {
            std::process::Command::new("git")
                .args(["checkout", "HEAD", "--", "."])
                .current_dir(&work_dir)
                .output()
        })
        .join()
        .map_err(|_| Error::Git("git checkout thread panicked".into()))?
        .map_err(|e| Error::Git(format!("git checkout failed: {e}")))?;

        if !output.status.success() {
            // Non-fatal: the commit succeeded, the working directory just wasn't updated.
            // Log but don't fail.
            tracing::warn!(
                "git checkout HEAD -- . failed after commit: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        Ok(commit_hex)
    }

    /// Get the HEAD commit hash as a hex string, or `None` if the repository
    /// is empty (no commits yet).
    pub fn head_hash(&self) -> Result<Option<String>> {
        let head = self
            .inner
            .head()
            .map_err(|e| Error::Git(format!("failed to get HEAD: {}", e)))?;

        if head.is_unborn() {
            return Ok(None);
        }

        match head.into_peeled_id() {
            Ok(id) => Ok(Some(id.to_hex().to_string())),
            Err(e) => Err(Error::Git(format!("failed to peel HEAD: {}", e))),
        }
    }
}
