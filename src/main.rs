mod condense;
mod extract;
mod index;
mod ownership;
mod types;

use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "cargo-llm-context",
    bin_name = "cargo llm-context",
    about = "Generate condensed, ownership-annotated API context files for LLM agents",
    long_about = "Parses a Rust crate's public API using syn and emits per-module markdown \
                  files annotated with ownership semantics. Intended to be fed to an LLM \
                  agent before it edits that module.\n\n\
                  Output: one <module-path>.md per module + _index.md for navigation."
)]
struct Args {
    /// Path to the crate root (directory containing Cargo.toml).
    #[arg(default_value = ".")]
    path: PathBuf,

    /// Crate name, required when running from a workspace root.
    #[arg(long, value_name = "NAME")]
    krate: Option<String>,

    /// Directory to write context files into.
    #[arg(long, default_value = "ai-context", value_name = "DIR")]
    output_dir: PathBuf,

    /// Also write a single _merged.md concatenating all module files.
    #[arg(long)]
    merge: bool,

    /// Approximate token budget per module file (1 token ≈ 4 chars).
    #[arg(long, default_value = "800", value_name = "N")]
    token_budget: usize,
}

fn main() -> Result<()> {
    // When invoked as `cargo llm-context`, cargo passes "llm-context" as argv[1].
    // Strip it so clap sees the remaining args normally.
    let raw: Vec<_> = std::env::args_os().collect();
    let skip_subcommand = raw
        .get(1)
        .and_then(|s| s.to_str())
        .map(|s| s == "llm-context")
        .unwrap_or(false);

    let args = if skip_subcommand {
        Args::parse_from(raw[0..1].iter().chain(raw[2..].iter()))
    } else {
        Args::parse()
    };

    run(args)
}

fn run(args: Args) -> Result<()> {
    eprintln!("extracting crate at {} ...", args.path.display());
    let modules = extract::extract_crate(&args.path, args.krate.as_deref())?;

    if modules.is_empty() {
        eprintln!("warning: no public items found");
        return Ok(());
    }

    std::fs::create_dir_all(&args.output_dir)?;

    // Per-module context files
    for module in &modules {
        let filename = module.path.replace("::", "-") + ".md";
        let content = condense::render_module(module, args.token_budget);
        let out_path = args.output_dir.join(&filename);
        std::fs::write(&out_path, &content)?;
        eprintln!(
            "  wrote {} ({} items, ~{} tokens)",
            filename,
            module.items.len(),
            content.len() / 4
        );
    }

    // Navigation index
    let index = index::render_index(&modules);
    let index_path = args.output_dir.join("_index.md");
    std::fs::write(&index_path, &index)?;
    eprintln!("  wrote _index.md ({} modules)", modules.len());

    // Optional merged file
    if args.merge {
        let merged = modules
            .iter()
            .map(|m| condense::render_module(m, args.token_budget))
            .collect::<Vec<_>>()
            .join("\n\n---\n\n");
        std::fs::write(args.output_dir.join("_merged.md"), &merged)?;
        eprintln!("  wrote _merged.md (~{} tokens total)", merged.len() / 4);
    }

    eprintln!("done.");
    Ok(())
}
