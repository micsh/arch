use crate::context::{ArchContext, print_json};
use crate::depgraph;
use std::collections::{HashMap, HashSet};

pub fn run(json: bool) -> Result<(), String> {
    let ctx = ArchContext::load()?;

    let stories = match ctx.load_stories()? {
        Some(s) => s,
        None => {
            println!("📖 No stories.yaml found in architecture/");
            return Ok(());
        }
    };

    if stories.stories.is_empty() {
        println!("📖 No stories defined");
        return Ok(());
    }

    let index = ctx.build_index();
    let actual_deps = depgraph::build_dep_graph(&ctx, &index, true);

    let container_ids: HashSet<String> = ctx.arch.containers
        .iter().map(|c| c.id.to_lowercase()).collect();
    let all_module_ids: HashSet<String> = actual_deps.keys().cloned().collect();

    let mut story_results: Vec<serde_json::Value> = Vec::new();
    let mut total_stories = 0;
    let mut passed_stories = 0;

    for story in &stories.stories {
        total_stories += 1;
        let desc = story.description.trim().replace('\n', " ");

        if story.flow.len() < 2 {
            story_results.push(serde_json::json!({
                "id": story.id, "description": desc,
                "status": "warning", "message": "Flow has fewer than 2 steps",
            }));
            if !json {
                let desc_display = truncate_desc(&desc);
                println!("📖 {} — {}", story.id, desc_display);
                println!("  ⚠️  Flow has fewer than 2 steps\n");
            }
            continue;
        }

        let mut unknown_steps: Vec<String> = Vec::new();
        for step in &story.flow {
            let lower = step.to_lowercase();
            if !all_module_ids.contains(&lower) && !container_ids.contains(&lower) {
                unknown_steps.push(step.clone());
            }
        }
        if !unknown_steps.is_empty() {
            story_results.push(serde_json::json!({
                "id": story.id, "description": desc,
                "status": "error", "unknown_steps": unknown_steps,
            }));
            if !json {
                let desc_display = truncate_desc(&desc);
                println!("📖 {} — {}", story.id, desc_display);
                for s in &unknown_steps { println!("  ❌ Unknown: {s}"); }
                println!();
            }
            continue;
        }

        let (connections, gaps, skips, pairs) =
            verify_story_pairs(&story.flow, &actual_deps, json);

        let passed = gaps == 0;
        if passed { passed_stories += 1; }

        story_results.push(serde_json::json!({
            "id": story.id, "description": desc,
            "status": if passed { "passed" } else { "failed" },
            "connections": connections, "gaps": gaps, "skips": skips,
            "pairs": pairs,
        }));

        if !json {
            let verified = connections + gaps;
            println!("  ✅ {connections}/{verified} connections verified");
            if skips > 0 { println!("  ⏭️  {skips} container ref(s) skipped"); }
            println!();
        }
    }

    if json {
        let output = serde_json::json!({
            "total": total_stories,
            "passed": passed_stories,
            "failed": total_stories - passed_stories,
            "stories": story_results,
        });
        print_json(&output)?;
        if passed_stories < total_stories {
            return Err(format!("{} story(ies) failed", total_stories - passed_stories));
        }
        return Ok(());
    }

    println!("📖 {passed_stories}/{total_stories} stories fully connected");
    Ok(())
}

fn verify_story_pairs(
    flow: &[String],
    actual_deps: &HashMap<String, HashSet<String>>,
    json: bool,
) -> (usize, usize, usize, Vec<serde_json::Value>) {
    let mut connections = 0;
    let mut gaps = 0;
    let mut skips = 0;
    let mut pairs: Vec<serde_json::Value> = Vec::new();

    for pair in flow.windows(2) {
        let from = &pair[0];
        let to = &pair[1];
        let from_lower = from.to_lowercase();
        let to_lower = to.to_lowercase();

        if !from_lower.contains('/') || !to_lower.contains('/') {
            skips += 1;
            pairs.push(serde_json::json!({"from": from, "to": to, "status": "skipped"}));
            if !json { println!("  {} → {}  ⏭️  container ref", from, to); }
            continue;
        }

        let has_dep = |from_id: &str, to_id: &str| -> bool {
            actual_deps
                .get(from_id)
                .map(|deps| {
                    deps.contains(to_id)
                        || deps.iter().any(|d| d.starts_with(&format!("{to_id}/")))
                })
                .unwrap_or(false)
        };

        let forward = has_dep(&from_lower, &to_lower);
        let reverse = has_dep(&to_lower, &from_lower);
        let from_container = from_lower.split('/').next().unwrap_or("");
        let to_container = to_lower.split('/').next().unwrap_or("");
        let same_container = from_container == to_container;

        if forward || reverse {
            connections += 1;
            pairs.push(serde_json::json!({"from": from, "to": to, "status": "connected"}));
            if !json { println!("  {} → {}  ✅", from, to); }
        } else if same_container {
            connections += 1;
            pairs.push(serde_json::json!({"from": from, "to": to, "status": "same_project"}));
            if !json { println!("  {} → {}  ✅ same project", from, to); }
        } else {
            gaps += 1;
            pairs.push(serde_json::json!({"from": from, "to": to, "status": "gap"}));
            if !json { println!("  {} → {}  ❌ no import found", from, to); }
        }
    }

    (connections, gaps, skips, pairs)
}

fn truncate_desc(desc: &str) -> String {
    if desc.len() > 80 { format!("{}…", &desc[..77]) } else { desc.to_string() }
}
