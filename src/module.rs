use anyhow::Result;
use std::collections::HashSet;
use std::path::Path;

use crate::config::{self, Module};

/// Resolved module with its name
pub struct ResolvedModule {
    pub name: String,
    pub module: Module,
}

/// Load all enabled modules for a profile, in dependency order
pub fn load_enabled_modules(project_dir: &Path, module_names: &[String]) -> Result<Vec<ResolvedModule>> {
    // Load all requested modules
    let mut modules: Vec<ResolvedModule> = Vec::new();
    let mut loaded = HashSet::new();

    for name in module_names {
        resolve_with_deps(project_dir, name, &mut modules, &mut loaded)?;
    }

    Ok(modules)
}

/// Recursively resolve a module and its dependencies in order
fn resolve_with_deps(
    project_dir: &Path,
    name: &str,
    result: &mut Vec<ResolvedModule>,
    loaded: &mut HashSet<String>,
) -> Result<()> {
    if loaded.contains(name) {
        return Ok(());
    }

    let module = config::load_module(project_dir, name)?;

    // Mark as loaded first to prevent circular deps
    loaded.insert(name.to_string());

    // Resolve dependencies first
    for dep in &module.depends {
        resolve_with_deps(project_dir, dep, result, loaded)?;
    }

    result.push(ResolvedModule {
        name: name.to_string(),
        module,
    });

    Ok(())
}

/// Validate modules for conflicts and integrity
pub fn validate_modules(_project_dir: &Path, modules: &[ResolvedModule]) -> Result<Vec<String>> {
    let mut warnings = Vec::new();
    let mut all_packages = HashSet::new();
    let mut all_removes = HashSet::new();

    for rm in modules {
        // Check for same package in install and remove
        for pkg in &rm.module.packages {
            if all_removes.contains(pkg) {
                warnings.push(format!(
                    "Module '{}' installs '{}' which is removed by another module",
                    rm.name, pkg
                ));
            }
            all_packages.insert(pkg.clone());
        }

        for pkg in &rm.module.remove {
            all_removes.insert(pkg.clone());
        }
    }

    Ok(warnings)
}
