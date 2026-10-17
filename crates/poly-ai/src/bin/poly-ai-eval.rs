//! `poly-ai-eval`: choose a local model with evidence.
//!
//! ```sh
//! poly-ai-eval list
//! poly-ai-eval run --model <name> [--samples tests/ai-samples.json] [--show]
//! poly-ai-eval try --model <name> "text to shorten"
//! ```
//!
//! This tool's output is the only place poly-ai prints message text, and
//! only the made-up sample texts (with `--show`) or the text given to `try`.

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use poly_ai::{AiError, LocalServer, Script, detect_servers, generate_decoy, shorten};
use serde::Deserialize;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Default samples file, relative to the workspace root.
const DEFAULT_SAMPLES: &str = "tests/ai-samples.json";
/// Percent scale for rates.
const PERCENT: f64 = 100.0;

#[derive(Parser)]
#[command(about = "Evaluate local LLMs for Polyphonia's Shorten and Generate decoy")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List detected local servers (LM Studio, Ollama) and their models.
    List,
    /// Run every sample through shorten, plus the decoy requests, and report.
    Run {
        #[arg(long)]
        model: String,
        #[arg(long, default_value = DEFAULT_SAMPLES)]
        samples: PathBuf,
        /// Also print the suggested texts.
        #[arg(long)]
        show: bool,
    },
    /// Shorten one text and print the suggestion.
    Try {
        #[arg(long)]
        model: String,
        text: String,
    },
}

#[derive(Deserialize)]
struct Samples {
    shorten: Vec<ShortenSample>,
    decoys: Vec<DecoySample>,
}

#[derive(Deserialize)]
struct ShortenSample {
    id: String,
    text: String,
}

#[derive(Deserialize)]
struct DecoySample {
    id: String,
    bytes: usize,
    script: String,
    tone: Option<String>,
}

fn parse_script(s: &str) -> Result<Script> {
    Ok(match s {
        "latin" => Script::Latin,
        "georgian" => Script::Georgian,
        "cyrillic" => Script::Cyrillic,
        other => bail!("unknown script {other:?}"),
    })
}

/// The first detected server that has this model.
fn server_with(model: &str) -> Result<LocalServer> {
    let servers = detect_servers();
    if servers.is_empty() {
        bail!("no local AI server found (start LM Studio's server or Ollama; see docs/ai.md)");
    }
    servers
        .into_iter()
        .find(|s| s.models.iter().any(|m| m == model))
        .with_context(|| format!("no detected server has model {model:?}; see `poly-ai-eval list`"))
}

fn why(e: &AiError) -> String {
    match e {
        AiError::Rejected { reasons } => reasons.join(", "),
        other => other.to_string(),
    }
}

fn average(total: Duration, n: usize) -> f64 {
    if n == 0 {
        0.0
    } else {
        total.as_secs_f64() / n as f64
    }
}

fn run(model: &str, samples: &PathBuf, show: bool) -> Result<()> {
    let samples: Samples = serde_json::from_str(
        &std::fs::read_to_string(samples)
            .with_context(|| format!("reading {}", samples.display()))?,
    )?;
    let server = server_with(model)?;
    println!(
        "server: {} ({:?})  model: {model}\n",
        server.base_url, server.kind
    );

    println!("shorten:");
    let (mut passed, mut saved_sum, mut latency) = (0usize, 0.0, Duration::ZERO);
    for s in &samples.shorten {
        let start = Instant::now();
        let result = shorten(&server, model, &s.text);
        let took = start.elapsed();
        latency += took;
        match result {
            Ok(r) => {
                passed += 1;
                saved_sum += r.saved_percent;
                println!(
                    "  PASS {:<14} {:>4} -> {:>4} B (-{:.0}%)  {:.1}s",
                    s.id,
                    r.original_bytes,
                    r.new_bytes,
                    r.saved_percent,
                    took.as_secs_f64()
                );
                if show {
                    println!("       {}", r.text);
                }
            }
            Err(e) => println!(
                "  FAIL {:<14} {}  {:.1}s",
                s.id,
                why(&e),
                took.as_secs_f64()
            ),
        }
    }
    let n = samples.shorten.len();
    println!(
        "  passed {passed}/{n}, average saved {:.1}% (of passing), average latency {:.1}s\n",
        if passed == 0 {
            0.0
        } else {
            saved_sum / passed as f64
        },
        average(latency, n)
    );

    println!("decoys:");
    let (mut fit, mut latency) = (0usize, Duration::ZERO);
    for d in &samples.decoys {
        let script = parse_script(&d.script)?;
        let start = Instant::now();
        let result = generate_decoy(&server, model, d.bytes, script, d.tone.clone());
        let took = start.elapsed();
        latency += took;
        match result {
            Ok(r) => {
                if r.fits {
                    fit += 1;
                }
                println!(
                    "  {} {:<14} target {:>4} B, got {:>4} B (allowed {}-{})  {:.1}s",
                    if r.fits { "FIT " } else { "MISS" },
                    d.id,
                    d.bytes,
                    r.text.len(),
                    r.allowed_len_range.0,
                    r.allowed_len_range.1,
                    took.as_secs_f64()
                );
                if show {
                    println!("       {}", r.text);
                }
            }
            Err(e) => println!(
                "  FAIL {:<14} {}  {:.1}s",
                d.id,
                why(&e),
                took.as_secs_f64()
            ),
        }
    }
    let n = samples.decoys.len();
    println!(
        "  fit rate {fit}/{n} ({:.0}%), average latency {:.1}s",
        if n == 0 {
            0.0
        } else {
            fit as f64 * PERCENT / n as f64
        },
        average(latency, n)
    );
    Ok(())
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::List => {
            let servers = detect_servers();
            if servers.is_empty() {
                println!("no local AI server found");
            }
            for s in servers {
                println!("{:?} at {}", s.kind, s.base_url);
                for m in s.models {
                    println!("  {m}");
                }
            }
        }
        Command::Run {
            model,
            samples,
            show,
        } => run(&model, &samples, show)?,
        Command::Try { model, text } => {
            let server = server_with(&model)?;
            let start = Instant::now();
            match shorten(&server, &model, &text) {
                Ok(r) => println!(
                    "{}\n({} -> {} bytes, -{:.0}%, {:.1}s)",
                    r.text,
                    r.original_bytes,
                    r.new_bytes,
                    r.saved_percent,
                    start.elapsed().as_secs_f64()
                ),
                Err(e) => bail!("rejected: {}", why(&e)),
            }
        }
    }
    Ok(())
}
