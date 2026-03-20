use crate::context::ArchContext;
use crate::llmcode;
use crate::lmindex;
use std::path::Path;

/// Resolve and display the context pack for a target file.
///
/// Discovers .llmcode files, loads arch context, builds a context pack,
/// and prints it to stdout as JSON (--json) or human-readable text.
pub fn run(file: &str, json: bool) -> Result<(), String> {
    let ctx = ArchContext::load()?;
    let mod_index = ctx.build_index();

    let llmcode_paths = llmcode::discover_llmcode_files(&ctx.root);
    let mut parsed_files = Vec::new();
    for path in &llmcode_paths {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Could not read '{}': {e}", path.display()))?;
        let parsed = llmcode::parse_llmcode_file(path, &content)?;
        parsed_files.push(parsed);
    }

    // Always rebuild in-memory (v0.6.0 spec — is_index_fresh used for future fast-path)
    let _ = lmindex::is_index_fresh(&ctx.root);

    let file_path = Path::new(file);
    let pack = lmindex::build_context_pack(&ctx, &parsed_files, file_path);
    let _ = mod_index; // Future: pass to build_context_pack for richer resolution

    if json {
        let output = serde_json::json!({
            "target_file": pack.target_file,
            "owner_module": pack.owner_module,
            "boundary": pack.boundary,
            "depends_on": pack.depends_on,
            "used_by": pack.used_by,
            "peer_modules": pack.peer_modules,
            "llmcode_blocks": pack.llmcode_blocks.iter().map(|b| serde_json::json!({
                "file": b.file,
                "mod_id": b.mod_id,
                "role": b.role,
                "inv": b.inv,
                "cntr_count": b.cntr_count,
            })).collect::<Vec<_>>(),
        });
        crate::context::print_json(&output)?;
        return Ok(());
    }

    // Human-readable output
    println!("🎯 Context pack: {}", pack.target_file);
    if let Some(ref module) = pack.owner_module {
        println!("   owner: {module}");
    } else {
        println!("   owner: (not mapped in architecture .arch files)");
    }
    if let Some(ref boundary) = pack.boundary {
        println!("   boundary: {boundary}");
    }
    if !pack.depends_on.is_empty() {
        println!("   depends_on: {}", pack.depends_on.join(", "));
    }
    if !pack.used_by.is_empty() {
        println!("   used_by: {}", pack.used_by.join(", "));
    }
    if !pack.peer_modules.is_empty() {
        println!("   peers: {}", pack.peer_modules.join(", "));
    }
    if !pack.llmcode_blocks.is_empty() {
        println!("   llmcode ({} block(s)):", pack.llmcode_blocks.len());
        for b in &pack.llmcode_blocks {
            if let Some(ref role) = b.role {
                println!("     {} — {role}", b.file);
            } else {
                println!("     {}", b.file);
            }
            for inv in &b.inv {
                println!("       ⚑ {inv}");
            }
        }
    }

    Ok(())
}
