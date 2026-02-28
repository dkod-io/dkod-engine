//! Shared validation helpers for gRPC request fields.

use tonic::Status;

/// Validate a client-supplied file path.
///
/// Rejects empty paths, absolute paths, null bytes, and `..` traversal
/// components. All gRPC handlers that accept a file path from the client
/// should call this before any further processing.
pub fn validate_file_path(path: &str) -> Result<(), Status> {
    if path.is_empty() {
        return Err(Status::invalid_argument("file path cannot be empty"));
    }
    if path.starts_with('/') || path.starts_with('\\') {
        return Err(Status::invalid_argument("file path must be relative"));
    }
    if path.contains('\0') {
        return Err(Status::invalid_argument("file path contains null byte"));
    }
    // Check for path traversal
    for component in path.split('/') {
        if component == ".." {
            return Err(Status::invalid_argument(
                "file path contains '..' traversal",
            ));
        }
    }
    Ok(())
}

/// Maximum allowed file content size (50 MB).
pub const MAX_FILE_SIZE: usize = 50 * 1024 * 1024;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_valid_relative_paths() {
        assert!(validate_file_path("src/main.rs").is_ok());
        assert!(validate_file_path("README.md").is_ok());
        assert!(validate_file_path("a/b/c/d.txt").is_ok());
        assert!(validate_file_path(".hidden").is_ok());
    }

    #[test]
    fn rejects_empty_path() {
        let err = validate_file_path("").unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
    }

    #[test]
    fn rejects_absolute_paths() {
        assert!(validate_file_path("/etc/passwd").is_err());
        assert!(validate_file_path("\\windows\\system32").is_err());
    }

    #[test]
    fn rejects_null_byte() {
        assert!(validate_file_path("src/\0evil.rs").is_err());
    }

    #[test]
    fn rejects_traversal() {
        assert!(validate_file_path("../etc/passwd").is_err());
        assert!(validate_file_path("src/../../etc/passwd").is_err());
        assert!(validate_file_path("foo/..").is_err());
    }

    #[test]
    fn allows_dots_that_are_not_traversal() {
        assert!(validate_file_path("src/.env").is_ok());
        assert!(validate_file_path("src/...").is_ok());
        assert!(validate_file_path(".gitignore").is_ok());
    }
}
