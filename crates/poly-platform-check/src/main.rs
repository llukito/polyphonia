//! `platform-check`: prepare sample files and check what messengers did to them.
//! See docs/platform-tests.md.

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use poly_platform_check::{RESULTS_FILE, Verdict, compare, prepare, render_markdown};

#[derive(Parser)]
#[command(
    name = "platform-check",
    version,
    about = "Check which messengers keep files intact"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Write sample.png, sample.wav, sample.mid and manifest.json into DIR
    Prepare { dir: PathBuf },
    /// Compare files in RECEIVED_ROOT/<platform>/<method>/ with the prepared samples
    Compare {
        prepared_dir: PathBuf,
        received_root: PathBuf,
    },
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Prepare { dir } => {
            let manifest = prepare(&dir)?;
            for f in &manifest.files {
                println!(
                    "{}  {}  {} bytes",
                    f.sha256,
                    dir.join(&f.name).display(),
                    f.size
                );
            }
            println!(
                "wrote {}",
                dir.join(poly_platform_check::MANIFEST_FILE).display()
            );
        }
        Command::Compare {
            prepared_dir,
            received_root,
        } => {
            let rows = compare(&prepared_dir, &received_root)?;
            let table = render_markdown(&rows);
            print!("{table}");
            let out = received_root.join(RESULTS_FILE);
            fs::write(&out, format!("# Platform check results\n\n{table}"))
                .with_context(|| format!("writing {}", out.display()))?;
            let identical = rows
                .iter()
                .filter(|r| r.verdict == Verdict::Identical)
                .count();
            let changed = rows
                .iter()
                .filter(|r| r.verdict == Verdict::Changed)
                .count();
            println!(
                "\n{identical} identical, {changed} changed, {} unmatched",
                rows.len() - identical - changed
            );
            println!("wrote {}", out.display());
        }
    }
    Ok(())
}
