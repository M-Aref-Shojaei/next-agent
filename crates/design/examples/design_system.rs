//! Generate `DESIGN.md` and `tokens.css` for a project from the command line,
//! with the same extractor as the studio's "Generate from project":
//!
//! ```sh
//! cargo run -p next-agent-design --example design_system -- <project-root> <name> <out-dir>
//! ```

use next_agent_design::system::{extract, render_design_md, render_tokens_css};
use std::path::Path;

fn main() -> Result<(), String> {
    let usage = "usage: design_system <project-root> <name> <out-dir>";
    let mut args = std::env::args().skip(1);
    let root = args.next().ok_or(usage)?;
    let name = args.next().ok_or(usage)?;
    let out = args.next().ok_or(usage)?;
    let system = extract(Path::new(&root), &name).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    std::fs::write(Path::new(&out).join("DESIGN.md"), render_design_md(&system))
        .map_err(|e| e.to_string())?;
    std::fs::write(
        Path::new(&out).join("tokens.css"),
        render_tokens_css(&system),
    )
    .map_err(|e| e.to_string())?;
    println!(
        "{} tokens from {} file(s)",
        system.tokens.len(),
        system.sources.len()
    );
    Ok(())
}
