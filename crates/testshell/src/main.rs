//! `cl-testshell` CLI.
#![forbid(unsafe_code)]
#![expect(clippy::print_stdout, reason = "CLI reports results on stdout")]

use std::path::PathBuf;

use cl_testshell::{compare_png, parse_viewport, render_blank};
use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "cl-testshell", version, about)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Debug, Subcommand)]
enum Cmd {
    /// Render an HTML file to PNG (M0: blank white canvas; input must exist).
    Render {
        input: PathBuf,
        #[arg(long)]
        png: PathBuf,
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

#[expect(clippy::disallowed_methods, reason = "binary entry point")]
fn main() -> anyhow::Result<()> {
    match Cli::parse().cmd {
        Cmd::Render {
            input,
            png,
            viewport,
        } => {
            anyhow::ensure!(input.is_file(), "input {} is not a file", input.display());
            let (w, h) = parse_viewport(&viewport)?;
            render_blank(w, h)?.save_png(&png)?;
            println!("rendered {}x{} -> {}", w, h, png.display());
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
