//! Module resolver (Gap 2 — Module System).
//!
//! Resolves `import { Name } from "path";` declarations by:
//! 1. Mapping the module path to a `.alk` file on disk.
//! 2. Parsing the file into a `ModuleDecl`.
//! 3. Collecting the module's exported function signatures.
//! 4. Merging them into the importing module's `FnSigTable`.
//!
//! # Path Resolution
//!
//! The module path `"std/canvas"` maps to `std/canvas.alk` relative to the
//! importing module's directory. If the file is not found, the resolver
//! returns an error.
//!
//! # Export Rules
//!
//! Only items declared with `pub` are exported. `pub fn`, `pub class`, and
//! `pub let` are visible to importing modules. Items without `pub` are
//! module-private and not importable.

#![forbid(unsafe_code)]

use core::fmt;
use std::collections::HashMap;
use std::path::PathBuf;

use crate::ast::{ItemDecl, ModuleDecl, Visibility};
use crate::parser::parse;
use crate::typechecker::{FnSig, FnSigTable};

/// An error during module resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolveError {
    /// Human-readable message.
    pub message: String,
    /// The module path that failed to resolve.
    pub module_path: String,
}

impl fmt::Display for ResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "module resolution error for '{}': {}",
            self.module_path, self.message
        )
    }
}

impl core::error::Error for ResolveError {}

/// A resolved module: its parsed AST and exported signatures.
struct ResolvedModule {
    /// The parsed module AST.
    /// The module's exported function signatures (pub items only).
    sigs: FnSigTable,
}

/// The module resolver. Caches resolved modules to avoid re-parsing.
pub struct ModuleResolver {
    /// Cache: module path → resolved module.
    cache: HashMap<String, ResolvedModule>,
    /// Base directory for relative path resolution.
    base_dir: PathBuf,
}

impl ModuleResolver {
    /// Create a new resolver with the given base directory.
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        Self {
            cache: HashMap::new(),
            base_dir: base_dir.into(),
        }
    }

    /// Resolve all imports in a module and merge their signatures into
    /// the provided `FnSigTable`.
    ///
    /// For each `import { Name } from "path";`:
    /// 1. Resolve "path" to a `.alk` file.
    /// 2. Parse the file.
    /// 3. Collect exported (pub) function signatures.
    /// 4. Insert them into `sigs` under the local name (or alias).
    pub fn resolve_imports(
        &mut self,
        module: &ModuleDecl,
        sigs: &mut FnSigTable,
    ) -> Result<(), ResolveError> {
        for imp in &module.imports {
            // Resolve the module path to a file.
            let resolved = self.resolve_module(&imp.module_path)?;

            // For each imported name, look it up in the resolved module's
            // signatures and insert it under the local name (or alias).
            for (name, alias) in &imp.names {
                let local_name = alias.as_ref().unwrap_or(name);
                if let Some(sig) = resolved.sigs.lookup(name) {
                    let mut import_sig = sig.clone();
                    import_sig.name = local_name.clone();
                    import_sig.imported_from = Some(imp.module_path.clone());
                    sigs.insert(local_name.clone(), import_sig);
                } else {
                    // Name not found in the resolved module — this is an error
                    // only if we actually loaded the file (not a built-in module).
                    // For built-in modules like "std/canvas", the names may be
                    // provided by the host at runtime, so we don't error.
                    if !imp.module_path.starts_with("std/") {
                        return Err(ResolveError {
                            message: format!(
                                "module '{}' does not export '{}'",
                                imp.module_path, name
                            ),
                            module_path: imp.module_path.clone(),
                        });
                    }
                }
            }
        }
        Ok(())
    }

    /// Resolve a module path to a parsed `ModuleDecl`, caching the result.
    fn resolve_module(&mut self, path: &str) -> Result<&ResolvedModule, ResolveError> {
        if self.cache.contains_key(path) {
            return Ok(self.cache.get(path).unwrap());
        }

        // Map the module path to a file path.
        let file_path = self.base_dir.join(format!("{}.alk", path));

        if !file_path.exists() {
            // For std/ modules, we allow them to be unresolved (host-provided).
            if path.starts_with("std/") {
                // Insert an empty resolved module so we don't keep trying.
                let empty_sigs = FnSigTable::new();
                self.cache.insert(
                    path.to_string(),
                    ResolvedModule {
                        sigs: empty_sigs,
                    },
                );
                return Ok(self.cache.get(path).unwrap());
            }
            return Err(ResolveError {
                message: format!("file not found: {}", file_path.display()),
                module_path: path.to_string(),
            });
        }

        // Read and parse the file.
        let src = std::fs::read_to_string(&file_path).map_err(|e| ResolveError {
            message: format!("failed to read {}: {}", file_path.display(), e),
            module_path: path.to_string(),
        })?;

        let decl = parse(&src).map_err(|e| ResolveError {
            message: format!("parse error in {}: {}", path, e),
            module_path: path.to_string(),
        })?;

        // Collect exported signatures.
        let mut sigs = FnSigTable::new();
        for item in &decl.items {
            match item {
                ItemDecl::Fn(f) if f.visibility == Visibility::Pub => {
                    sigs.insert(
                        f.name.clone(),
                        FnSig {
                            name: f.name.clone(),
                            params: f.params.iter().map(|p| p.ty.clone()).collect(),
                            return_type: f.return_type.clone(),
                            param_names: f.params.iter().map(|p| p.name.clone()).collect(),
                            receiver_class: None,
                            imported_from: None,
                        },
                    );
                }
                _ => {}
            }
        }

        // Recursively resolve the module's own imports.
        // (Depth-first; no cycle detection yet — a cycle would stack-overflow.)
        let mut nested_resolver = ModuleResolver {
            cache: std::mem::take(&mut self.cache),
            base_dir: self.base_dir.clone(),
        };
        let _ = nested_resolver.resolve_imports(&decl, &mut sigs);
        self.cache = std::mem::take(&mut nested_resolver.cache);

        self.cache.insert(
            path.to_string(),
            ResolvedModule {
                sigs,
            },
        );
        Ok(self.cache.get(path).unwrap())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolver_creates_with_base_dir() {
        let r = ModuleResolver::new("/tmp");
        assert_eq!(r.base_dir, PathBuf::from("/tmp"));
    }

    #[test]
    fn resolver_resolves_std_module_as_empty() {
        let mut r = ModuleResolver::new("/nonexistent");
        let m = ModuleDecl {
            name: "M".into(),
            scene: None,
            attributes: Vec::new(),
            items: Vec::new(),
            imports: vec![crate::ast::ImportDecl {
                module_path: "std/canvas".into(),
                names: vec![("draw".to_string(), None)],
                line: 1,
                col: 1,
            }],
            line: 1,
            col: 1,
        };
        let mut sigs = FnSigTable::new();
        // Should not error — std/ modules are allowed to be unresolved.
        let result = r.resolve_imports(&m, &mut sigs);
        assert!(result.is_ok(), "std/ imports should not error");
    }

    #[test]
    fn resolver_errors_on_missing_non_std_module() {
        let mut r = ModuleResolver::new("/nonexistent");
        let m = ModuleDecl {
            name: "M".into(),
            scene: None,
            attributes: Vec::new(),
            items: Vec::new(),
            imports: vec![crate::ast::ImportDecl {
                module_path: "mylib/utils".into(),
                names: vec![("helper".to_string(), None)],
                line: 1,
                col: 1,
            }],
            line: 1,
            col: 1,
        };
        let mut sigs = FnSigTable::new();
        let result = r.resolve_imports(&m, &mut sigs);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.message.contains("file not found"));
    }
}
