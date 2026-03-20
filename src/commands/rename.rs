use crate::llmcode;
use std::path::Path;

/// Migrate a module ID across all .llmcode files in the project.
///
/// Replaces every `MOD: <old_id>` occurrence with `MOD: <new_id>`.
/// Loads architecture context with `.ok()` — warns and proceeds if unavailable
/// (allows rename to run even when validate would fail).
///
/// ASSUMPTION: MOD: is the only field that carries the full module ID as an
/// authored value. CNTR: references use "module::symbol" form — the module
/// prefix there may also need updating, but that is left to the author for now.
/// IF INVALID: add CNTR: left/right rewriting.
pub fn run(old_id: &str, new_id: &str) -> Result<(), String> {
    let root = std::env::current_dir().map_err(|e| e.to_string())?;

    // Warn if architecture context is unavailable (don't block the rename)
    if crate::context::ArchContext::load().is_err() {
        eprintln!(
            "⚠️  arch context unavailable — proceeding with text rewrite only (run arch validate after)"
        );
    }

    let paths = llmcode::discover_llmcode_files(Path::new(&root));
    if paths.is_empty() {
        println!("No .llm files found — nothing to rename.");
        return Ok(());
    }

    let old_mod_line = format!("MOD: {old_id}");
    let new_mod_line = format!("MOD: {new_id}");

    let mut updated = 0usize;
    let mut files_changed = 0usize;

    for path in &paths {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Could not read '{}': {e}", path.display()))?;

        if !content.contains(&old_mod_line) {
            continue;
        }

        let new_content = content.replace(&old_mod_line, &new_mod_line);
        let count = content.matches(&old_mod_line).count();
        updated += count;
        files_changed += 1;

        std::fs::write(path, new_content)
            .map_err(|e| format!("Could not write '{}': {e}", path.display()))?;

        println!("  ✏️  {} ({count} occurrence(s))", path.display());
    }

    if files_changed == 0 {
        println!("No occurrences of 'MOD: {old_id}' found in .llm files.");
    } else {
        println!(
            "\n✅ Renamed '{old_id}' → '{new_id}' in {files_changed} file(s), {updated} occurrence(s)."
        );
        println!("   Run `arch validate` to confirm no dangling MOD: links remain.");
    }

    Ok(())
}
