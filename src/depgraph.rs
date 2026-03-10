use crate::context::{ArchContext, SOURCE_EXTENSIONS, walk_module_files};
use crate::imports;
use crate::resolve::{is_external_import, ModuleIndex};
use std::collections::{HashMap, HashSet};

/// Build the actual dependency graph from source files.
///
/// `resolve_self`: when true, includes same-container matches (use for stories);
/// when false, excludes same-container matches (use for drift/fitness).
///
/// Module IDs are normalised to lowercase for consistency. Architecture YAML IDs
/// are lowercase by convention, so no behavioral regression occurs.
///
/// Returns: module_id (lowercase) → set of module_ids (lowercase) it imports from.
pub fn build_dep_graph(
    ctx: &ArchContext,
    index: &ModuleIndex,
    resolve_self: bool,
) -> HashMap<String, HashSet<String>> {
    let explicitly_mapped = ctx.collect_mapped_files();
    let source_ext: HashSet<&str> = SOURCE_EXTENSIONS.iter().copied().collect();
    let mut actual_deps: HashMap<String, HashSet<String>> = HashMap::new();

    for container in &ctx.arch.containers {
        let detail = match ctx.details.get(&container.id) {
            Some(d) => d,
            None => continue,
        };

        for module in &detail.modules {
            let full_id = format!("{}/{}", container.id, module.id).to_lowercase();
            let files_to_scan = walk_module_files(
                &ctx.root, &container.path, module,
                &explicitly_mapped, &source_ext, &ctx.ignore_patterns,
            );

            let mut deps = HashSet::new();
            for scan_path in &files_to_scan {
                let file_content = match std::fs::read_to_string(scan_path) {
                    Ok(c) => c,
                    Err(_) => continue,
                };
                let file_imports = imports::extract_imports(scan_path, &file_content);
                for imp in &file_imports {
                    if is_external_import(&imp.raw) {
                        continue;
                    }
                    let resolved = if resolve_self {
                        index.resolve_all(&imp.raw, &container.id)
                    } else {
                        index.resolve(&imp.raw, &container.id)
                    };
                    for target in &resolved {
                        let target_lower = target.to_lowercase();
                        if target_lower != full_id {
                            deps.insert(target_lower);
                        }
                    }
                }
            }
            actual_deps.insert(full_id, deps);
        }
    }

    actual_deps
}
