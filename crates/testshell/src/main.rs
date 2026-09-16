//! `cl-testshell` CLI.
#![forbid(unsafe_code)]
#![expect(clippy::print_stdout, reason = "CLI reports results on stdout")]

use std::path::PathBuf;

use cl_testshell::reftest;
use cl_testshell::{RenderOptions, RenderOutput, Stage, compare_png, parse_viewport, select};
use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "cl-testshell", version, about)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Debug, Subcommand)]
enum Cmd {
    /// Render an HTML document to PNG through the real M1a pipeline.
    Render {
        /// A filesystem path, or an absolute `file:` URL (recognised by the `file:` prefix).
        input: String,
        #[arg(long)]
        png: PathBuf,
        #[arg(long, default_value = "800x600")]
        viewport: String,
    },
    /// Print one pipeline stage's dump to stdout: `dom`, `style`, `box-tree`, `fragments`,
    /// or `display-list`.
    Dump {
        /// A filesystem path, or an absolute `file:` URL (recognised by the `file:` prefix).
        input: String,
        #[arg(long)]
        stage: String,
        #[arg(long, default_value = "800x600")]
        viewport: String,
    },
    /// Compare two PNGs; exit 0 if differing pixels <= max-diff-pixels.
    Compare {
        a: PathBuf,
        b: PathBuf,
        #[arg(long, default_value_t = 0)]
        max_diff_pixels: u64,
    },
    /// Run every `<name>.html`/`<name>-ref.html` reftest pair under `--dir`, printing one
    /// `PASS`/`FAIL` line per pair; exits 1 if any pair failed (including a `FAIL (known)`).
    Reftest {
        /// Directory to discover pairs in. Defaults to this crate's own `tests/ref`.
        #[arg(long)]
        dir: Option<PathBuf>,
        /// Only run pairs whose name contains this substring.
        #[arg(long)]
        filter: Option<String>,
    },
}

/// Renders `input` against `viewport`.
///
/// `input` is a `file:` URL if it starts with `file:` (loaded through [`cl_net::load_file`]
/// and handed to [`cl_testshell::render_bytes`]), otherwise a filesystem path (handed to
/// [`cl_testshell::render_file`], which resolves it relative to the current directory).
fn render_input(input: &str, viewport: &str) -> anyhow::Result<RenderOutput> {
    let opts = RenderOptions {
        viewport: parse_viewport(viewport)?,
    };
    if input.starts_with("file:") {
        let url = cl_net::Url::parse(input)?;
        let bytes = cl_net::load_file(&url)?;
        Ok(cl_testshell::render_bytes(&bytes, &url, &opts)?)
    } else {
        Ok(cl_testshell::render_file(&PathBuf::from(input), &opts)?)
    }
}

/// Prints every collected [`cl_style::SheetWarning`] line to stderr — never fatal (see
/// [`cl_testshell::RenderOutput::warnings`]'s docs), just surfaced so a broken `<link>`
/// doesn't render silently.
fn report_warnings(output: &RenderOutput) {
    for warning in &output.warnings {
        eprintln!("warning: {warning}");
    }
}

#[expect(clippy::disallowed_methods, reason = "binary entry point")]
fn main() -> anyhow::Result<()> {
    match Cli::parse().cmd {
        Cmd::Render {
            input,
            png,
            viewport,
        } => {
            let output = render_input(&input, &viewport)?;
            report_warnings(&output);
            output.pixmap.save_png(&png)?;
            println!("rendered {input} -> {}", png.display());
            Ok(())
        }
        Cmd::Dump {
            input,
            stage,
            viewport,
        } => {
            let output = render_input(&input, &viewport)?;
            report_warnings(&output);
            let stage = Stage::parse(&stage)?;
            println!("{}", select(&output.stages, stage));
            Ok(())
        }
        Cmd::Compare {
            a,
            b,
            max_diff_pixels,
        } => {
            let d = compare_png(&a, &b)?;
            println!(
                "differing_pixels={} size={}x{}",
                d.differing_pixels, d.width, d.height
            );
            if d.differing_pixels > max_diff_pixels {
                std::process::exit(1);
            }
            Ok(())
        }
        Cmd::Reftest { dir, filter } => {
            let dir = dir.unwrap_or_else(reftest::default_dir);
            let results = reftest::run(&dir, filter.as_deref(), &reftest::default_failures_dir())?;
            let mut any_failed = false;
            for result in &results {
                println!("{}", result.line());
                any_failed |= result.is_failure();
            }
            let passed = results.iter().filter(|r| !r.is_failure()).count();
            println!("== {passed}/{} passed ==", results.len());
            if any_failed {
                std::process::exit(1);
            }
            Ok(())
        }
    }
}
