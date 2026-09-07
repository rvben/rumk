//! Read-only contributor tooling: cargo run --example prerequisite-coverage -- Makefile
use anyhow::Result;
use clap::Parser;
use rumk::project::{Project, ProjectOptions};
use rumk::rules::prerequisites::coverage;
use std::path::PathBuf;

#[derive(Parser)]
struct Args {
    #[arg(required = true)]
    paths: Vec<PathBuf>,
}

fn main() -> Result<()> {
    let mut roots = Vec::new();
    for path in Args::parse().paths {
        let project = Project::load(&path, &ProjectOptions::default())?;
        roots.push(serde_json::json!({"root": path, "coverage": coverage(&project)}));
    }
    println!("{}", serde_json::to_string_pretty(&roots)?);
    Ok(())
}
