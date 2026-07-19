use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const MARKER_FILE: &str = ".dcli-bootc";
const REGISTRY_DIR: &str = "dcli-bootc";
const REGISTRY_FILE: &str = "projects.toml";

/// The project registry stored at ~/.config/dcli-bootc/projects.toml
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Registry {
    default: Option<String>,
    projects: std::collections::HashMap<String, ProjectEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProjectEntry {
    name: String,
    path: String,
}

/// Resolve the project path:
/// 1. If --project flag was given, use it
/// 2. If current directory has a .dcli-bootc marker, use it
/// 3. Fall back to the default project in the registry
pub fn resolve_project(cli_project: Option<String>) -> Result<PathBuf> {
    // Explicit --project flag
    if let Some(path) = cli_project {
        let p = PathBuf::from(&path);
        if !p.join(MARKER_FILE).exists() {
            anyhow::bail!("No bootc project found at {} (missing {})", path, MARKER_FILE);
        }
        return Ok(p.canonicalize()?);
    }

    // Check current directory for marker
    let cwd = std::env::current_dir()?;
    if cwd.join(MARKER_FILE).exists() {
        return Ok(cwd);
    }

    // Walk up parent dirs looking for marker (like git)
    let mut parent = cwd.parent();
    while let Some(dir) = parent {
        if dir.join(MARKER_FILE).exists() {
            return Ok(dir.to_path_buf());
        }
        parent = dir.parent();
    }

    // Fall back to registry default
    let registry = load_registry()?;
    if let Some(default_path) = registry.default {
        let p = PathBuf::from(&default_path);
        if p.exists() {
            return Ok(p);
        }
    }

    anyhow::bail!(
        "No bootc project found in current directory or parent paths.\n\
         Run 'dcli-bootc init <name>' to create one, or use --project <path>"
    );
}

/// Get or create the registry directory
fn registry_dir() -> Result<PathBuf> {
    let base = dirs::config_dir()
        .context("Could not find XDG config directory")?;
    Ok(base.join(REGISTRY_DIR))
}

/// Load the project registry from disk
fn load_registry() -> Result<Registry> {
    let path = registry_dir()?.join(REGISTRY_FILE);
    if !path.exists() {
        return Ok(Registry::default());
    }
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read registry: {}", path.display()))?;
    let registry: Registry = toml::from_str(&content)
        .with_context(|| format!("Failed to parse registry: {}", path.display()))?;
    Ok(registry)
}

/// Save the project registry to disk
fn save_registry(registry: &Registry) -> Result<()> {
    let dir = registry_dir()?;
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(REGISTRY_FILE);
    let content = toml::to_string_pretty(registry)?;
    std::fs::write(&path, content)?;
    Ok(())
}

/// Register a new project in the registry (or update existing)
pub fn register_project(path: &Path, name: &str, set_default: bool) -> Result<()> {
    let canonical = path.canonicalize()?;
    let path_str = canonical.to_string_lossy().to_string();
    let mut registry = load_registry()?;

    registry.projects.insert(
        path_str.clone(),
        ProjectEntry {
            name: name.to_string(),
            path: path_str.clone(),
        },
    );

    if set_default || registry.default.is_none() {
        registry.default = Some(path_str);
    }

    save_registry(&registry)?;
    Ok(())
}

/// Get the stored project name from the registry
pub fn get_project_name(path: &Path) -> Result<String> {
    let canonical = path.canonicalize()?;
    let path_str = canonical.to_string_lossy().to_string();
    let registry = load_registry()?;

    if let Some(entry) = registry.projects.get(&path_str) {
        return Ok(entry.name.clone());
    }

    // Fall back to directory name
    Ok(path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string()))
}

/// Create the .dcli-bootc marker file in the project directory
pub fn create_marker(path: &Path) -> Result<()> {
    let marker = path.join(MARKER_FILE);
    if !marker.exists() {
        std::fs::write(&marker, "")?;
    }
    Ok(())
}
