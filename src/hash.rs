//! Content hashing of build inputs for cache-busting container builds.

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::path::Path;
use walkdir::WalkDir;

/// Directories whose contents are bind-mounted into the container build and
/// are therefore invisible to podman's layer cache key.
const HASHED_DIRS: &[&str] = &["build_files", "files"];

/// Compute a stable SHA-256 over the contents of every build input directory.
///
/// The generated Containerfile references this hash in its RUN instruction, so
/// changing any generated script or overlay file invalidates exactly the
/// customization layer instead of silently reusing a stale cached one.
pub fn compute_build_hash(project_dir: &Path) -> Result<String> {
    let mut hasher = Sha256::new();

    for sub in HASHED_DIRS {
        let dir = project_dir.join(sub);
        if !dir.is_dir() {
            continue;
        }

        let mut paths: Vec<_> = WalkDir::new(&dir)
            .into_iter()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_type().is_file())
            .map(|entry| entry.into_path())
            .collect();
        paths.sort();

        for path in paths {
            let rel = path.strip_prefix(project_dir).unwrap_or(&path);
            hasher.update(rel.to_string_lossy().as_bytes());
            hasher.update(b"\0");
            let content = std::fs::read(&path)
                .with_context(|| format!("Failed to read {}", path.display()))?;
            hasher.update(content);
            hasher.update(b"\0");
        }
    }

    Ok(format!("{:x}", hasher.finalize()))
}
