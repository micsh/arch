pub mod arch_parser;
mod commands;
mod depgraph;
mod llmcode;
mod lmindex;
mod schema;
mod scanner;
mod imports;
mod resolve;
pub mod context;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "arch", version, about = "Architecture-as-code for any codebase")]
struct Cli {
    /// Output results as JSON instead of human-readable text
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Scan project structure and generate initial architecture .arch files
    Init {
        /// Deep scan: infer modules from .csproj ProjectReference tags and Python packages
        #[arg(long)]
        deep: bool,
    },
    /// Check .arch integrity: files exist, cross-refs valid, schema correct
    Validate,
    /// List source files not mapped to any module
    Coverage,
    /// Find which module owns a concept
    Owns {
        /// The concept to look up
        concept: String,
    },
    /// Check architecture health: validate .arch integrity + find unmapped source files
    Stale,
    /// Compare declared dependencies against actual code imports
    Drift {
        /// Also report declared dependencies that have no matching actual imports (reverse drift)
        #[arg(long)]
        stale_declared: bool,
    },
    /// Validate architectural rules against actual code
    Fitness,
    /// Show fitness rules from system.arch, optionally filtered to a module
    Rules {
        /// Module or container name to filter rules (e.g. context, imports, resolve)
        module: Option<String>,
    },
    /// Verify story flows against actual import connections
    Stories,
    /// Compile arch.index.json and llmcode.index.json
    Index,
    /// Display spec version(s) understood by this binary
    Spec {
        /// Show the full llmcode grammar spec
        #[arg(long)]
        llmcode: bool,
        /// Show the full .arch grammar spec
        #[arg(long)]
        arch: bool,
    },
    /// Rename a module ID across all .llmcode files
    Rename {
        /// Old module ID (e.g. commands/old-name)
        old_id: String,
        /// New module ID (e.g. commands/new-name)
        new_id: String,
    },
    /// Resolve and display the context pack for a source file
    Context {
        /// Source file path to resolve context for
        #[arg(long)]
        file: String,
    },
    /// Generate Mermaid diagrams from architecture .arch files
    Mermaid {
        /// Generate story flow diagrams instead of container diagram
        #[arg(long)]
        stories: bool,
        /// Use brief labels (IDs only, no descriptions)
        #[arg(long)]
        brief: bool,
        /// Generate C4 Container diagram
        #[arg(long)]
        c4: bool,
        /// Render to SVG file (requires mmdc / @mermaid-js/mermaid-cli)
        #[arg(long, default_missing_value = "architecture.svg", num_args = 0..=1)]
        svg: Option<String>,
        /// Inject diagram into README.md between <!-- arch:mermaid:start/end --> markers
        #[arg(long)]
        update_readme: bool,
    },
}

fn main() {
    let cli = Cli::parse();
    let json = cli.json;

    let result = match cli.command {
        Commands::Init { deep } => commands::init::run(deep),
        Commands::Validate => commands::validate::run(json),
        Commands::Coverage => commands::coverage::run(json),
        Commands::Owns { concept } => commands::owns::run(&concept, json),
        Commands::Stale => commands::stale::run(json),
        Commands::Drift { stale_declared } => commands::drift::run(json, stale_declared),
        Commands::Fitness => commands::fitness::run(json),
        Commands::Rules { module } => commands::rules::run(module.as_deref(), json),
        Commands::Stories => commands::stories::run(json),
        Commands::Index => commands::index::run(json),
        Commands::Spec { llmcode, arch } => commands::spec::run(llmcode, arch),
        Commands::Rename { old_id, new_id } => commands::rename::run(&old_id, &new_id),
        Commands::Context { file } => commands::contextpack::run(&file, json),
        Commands::Mermaid { stories, brief, c4, svg, update_readme } =>
            commands::mermaid::run(stories, brief, c4, svg, update_readme),
    };

    if let Err(e) = result {
        if json {
            let err = serde_json::json!({ "error": e });
            if let Err(je) = context::print_json(&err) {
                eprintln!("Error: {e}\n(JSON serialization also failed: {je})");
            }
        } else {
            eprintln!("Error: {e}");
        }
        std::process::exit(1);
    }
}
