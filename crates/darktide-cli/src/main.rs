use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::fs;
use std::path::{Path, PathBuf};

use darktide_bundle::hash::{lookup_extension, murmur_hash64};
use darktide_bundle::{Bundle, Oodle};

#[derive(Parser)]
#[command(name = "darktide-cli")]
#[command(about = "Extract files from Darktide resource bundles")]
#[command(version)]
struct Cli {
    /// Path to the Oodle shared library
    #[arg(long, global = true, default_value = "liboo2corelinux64.so.9")]
    oodle_lib: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// List files in a bundle
    List {
        /// Path to bundle file
        #[arg(short, long)]
        input: PathBuf,

        /// Filter by extension (e.g. "lua", "texture")
        extension: Option<String>,
    },

    /// Extract files from a bundle
    Extract {
        /// Path to bundle file
        #[arg(short, long)]
        input: PathBuf,

        /// Output directory
        #[arg(short, long)]
        output: PathBuf,

        /// Filter by extension (e.g. "lua", "texture")
        extension: Option<String>,

        /// Extract raw data without file name resolution
        #[arg(long)]
        raw: bool,
    },

    /// Dump all extension/name hashes from a bundle
    DumpHashes {
        /// Path to bundle file
        #[arg(short, long)]
        input: PathBuf,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::List { input, extension } => {
            cmd_list(&input, extension.as_deref())?;
        }
        Commands::Extract {
            input,
            output,
            extension,
            raw,
        } => {
            let oodle = load_oodle(&cli.oodle_lib)?;
            cmd_extract(&input, &output, extension.as_deref(), raw, &oodle)?;
        }
        Commands::DumpHashes { input } => {
            cmd_dump_hashes(&input)?;
        }
    }

    Ok(())
}

fn load_oodle(path: &str) -> Result<Oodle> {
    Oodle::load(path).map_err(|e| anyhow::anyhow!("Failed to load Oodle library '{}': {}", path, e))
}

fn open_bundle(path: &Path) -> Result<Bundle> {
    Bundle::open(path.to_str().context("Invalid bundle path")?)
        .with_context(|| format!("Failed to open bundle: {}", path.display()))
}

/// List files in a bundle (index only, no decompression needed).
fn cmd_list(input: &Path, extension_filter: Option<&str>) -> Result<()> {
    let mut bundle = open_bundle(input)?;
    let index = bundle.read_index()?;

    let ext_hash_filter = extension_filter.map(|e| murmur_hash64(e.as_bytes()));

    let mut count = 0u64;
    for entry in &index {
        if let Some(hash) = ext_hash_filter {
            if entry.ext != hash {
                continue;
            }
        }

        let ext_name = lookup_extension(entry.ext)
            .unwrap_or("unknown")
            .to_string();

        println!(
            "{:016x}\t{:016x}\t{}\tmode={}",
            entry.ext, entry.name, ext_name, entry.mode
        );
        count += 1;
    }

    eprintln!("{} files listed", count);
    Ok(())
}

/// Extract files from a bundle.
fn cmd_extract(
    input: &Path,
    output: &Path,
    extension_filter: Option<&str>,
    raw: bool,
    oodle: &Oodle,
) -> Result<()> {
    let mut bundle = open_bundle(input)?;
    let files = bundle.extract_files(oodle)?;

    fs::create_dir_all(output)?;

    let ext_hash_filter = extension_filter.map(|e| murmur_hash64(e.as_bytes()));

    let mut count = 0u64;
    let mut total_bytes = 0u64;

    for file in &files {
        if let Some(hash) = ext_hash_filter {
            if file.ext != hash {
                continue;
            }
        }

        let ext_name = lookup_extension(file.ext).unwrap_or("unknown");

        if raw {
            // Raw mode: dump to <name_hash>.<ext_hash>
            let filename = format!("{:016x}.{:016x}", file.name, file.ext);
            let out_path = output.join(&filename);
            fs::write(&out_path, &file.data)?;
        } else {
            // Named mode: dump to <ext>/<name_hash>
            let dir = output.join(ext_name);
            fs::create_dir_all(&dir)?;
            let filename = format!("{:016x}", file.name);
            let out_path = dir.join(&filename);
            fs::write(&out_path, &file.data)?;
        }

        total_bytes += file.data.len() as u64;
        count += 1;
    }

    eprintln!(
        "Extracted {} files ({} bytes) to {}",
        count,
        total_bytes,
        output.display()
    );
    Ok(())
}

/// Dump all hashes from the bundle index with resolved extension names.
fn cmd_dump_hashes(input: &Path) -> Result<()> {
    let mut bundle = open_bundle(input)?;
    let index = bundle.read_index()?;

    println!("ext_hash\t\t\tname_hash\t\t\text\tmode");

    for entry in &index {
        let ext_name = lookup_extension(entry.ext)
            .unwrap_or("unknown")
            .to_string();
        println!(
            "{:016x}\t{:016x}\t{}\t{}",
            entry.ext, entry.name, ext_name, entry.mode
        );
    }

    eprintln!("{} entries", index.len());
    Ok(())
}
