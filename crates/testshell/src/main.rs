//! `cl-testshell` CLI.
#![forbid(unsafe_code)]
#![expect(clippy::print_stdout, reason = "CLI reports results on stdout")]

use std::path::PathBuf;

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
    }
}
