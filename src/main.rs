use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::collections::HashSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

use darktide_bundle::hash::{lookup_extension, murmur_hash64};
use darktide_bundle::lua::extract_chunkname;
use darktide_bundle::{normalize_luajit, scan_strings, Bundle, Dictionary, Oodle};

/// Join `output` with an untrusted path, rejecting components that would escape
/// the output directory (absolute roots, `..`, Windows drive prefixes).
/// Returns `None` if the path is unsafe.
fn safe_join(output: &Path, untrusted: &str) -> Option<PathBuf> {
    let p = Path::new(untrusted);
    if p.components().any(|c| {
        matches!(
            c,
            Component::RootDir | Component::ParentDir | Component::Prefix(_)
        )
    }) {
        return None;
    }
    Some(output.join(p))
}

#[cfg(target_os = "windows")]
const DEFAULT_OODLE_LIB: &str = "oo2core_9_win64.dll";

#[cfg(not(target_os = "windows"))]
const DEFAULT_OODLE_LIB: &str = "liboo2corelinux64.so.9";

#[derive(Parser)]
#[command(name = "dtex")]
#[command(about = "Extract files from Darktide resource bundles")]
#[command(version)]
struct Cli {
    /// Path to the Oodle shared library
    #[arg(long, global = true, default_value = DEFAULT_OODLE_LIB)]
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

        /// Path to dictionary file for name resolution
        #[arg(long)]
        dictionary: Option<PathBuf>,

        /// Name Lua files using their chunkname from bytecode debug info
        #[arg(long)]
        lua_chunknames: bool,
    },

    /// Dump all extension/name hashes from a bundle
    DumpHashes {
        /// Path to bundle file
        #[arg(short, long)]
        input: PathBuf,
    },

    /// Scan bundle content for path strings and build a dictionary
    Scan {
        /// Path to bundle file (can be repeated for multiple bundles)
        #[arg(short, long, num_args = 1..)]
        input: Vec<PathBuf>,

        /// Output dictionary file (one path per line)
        #[arg(short, long, default_value = "dictionary.txt")]
        output: PathBuf,

        /// Existing dictionary to merge with (append mode)
        #[arg(long)]
        merge: Option<PathBuf>,
    },

    /// Check dictionary coverage against bundle index hashes
    Coverage {
        /// Path to dictionary file
        #[arg(short, long)]
        dictionary: PathBuf,

        /// Path to bundle file (can be repeated for multiple bundles)
        #[arg(short, long, num_args = 1..)]
        input: Vec<PathBuf>,
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
            dictionary,
            lua_chunknames,
        } => {
            let oodle = load_oodle(&cli.oodle_lib)?;
            let dict = if let Some(dict_path) = dictionary {
                Some(Dictionary::load(
                    dict_path.to_str().context("Invalid dictionary path")?,
                )?)
            } else {
                None
            };
            cmd_extract(
                &input,
                &output,
                extension.as_deref(),
                raw,
                lua_chunknames,
                &oodle,
                dict.as_ref(),
            )?;
        }
        Commands::DumpHashes { input } => {
            cmd_dump_hashes(&input)?;
        }
        Commands::Scan {
            input,
            output,
            merge,
        } => {
            let oodle = load_oodle(&cli.oodle_lib)?;
            cmd_scan(&input, &output, merge.as_deref(), &oodle)?;
        }
        Commands::Coverage { dictionary, input } => {
            cmd_coverage(&dictionary, &input)?;
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

        let ext_name = lookup_extension(entry.ext).unwrap_or("unknown").to_string();

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
    lua_chunknames: bool,
    oodle: &Oodle,
    dictionary: Option<&Dictionary>,
) -> Result<()> {
    let mut bundle = open_bundle(input)?;
    let files = bundle.extract_files(oodle)?;

    fs::create_dir_all(output)?;

    let ext_hash_filter = extension_filter.map(|e| murmur_hash64(e.as_bytes()));

    let mut count = 0u64;
    let mut total_bytes = 0u64;
    let mut unresolved = 0u64;
    let mut unnamed_lua = 0u64;

    for file in &files {
        if let Some(hash) = ext_hash_filter {
            if file.ext != hash {
                continue;
            }
        }

        let ext_name = lookup_extension(file.ext).unwrap_or("unknown");

        // Lua files are unconditionally normalized to standard LuaJIT bytecode
        // (strips the 24-byte Fatshark prefix, restores `LJ` magic and `0x02` version).
        let bytes = if ext_name == "lua" {
            normalize_luajit(&file.data)
        } else {
            std::borrow::Cow::Borrowed(file.data.as_slice())
        };

        // Try lua chunkname naming when flag is set and file is lua
        if lua_chunknames && ext_name == "lua" {
            if let Some(chunkname) =
                extract_chunkname(&file.data).and_then(|cn| safe_join(output, &cn))
            {
                let out_path = chunkname;
                if let Some(parent) = out_path.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::write(&out_path, &bytes)
                    .with_context(|| format!("writing {}", out_path.display()))?;
            } else {
                // No chunkname found or unsafe path — place in unnamed/ with hash-based name
                let dir = output.join("unnamed");
                fs::create_dir_all(&dir)?;
                let filename = format!("{:016x}", file.name);
                let out_path = dir.join(&filename);
                fs::write(&out_path, &bytes)
                    .with_context(|| format!("writing {}", out_path.display()))?;
                unnamed_lua += 1;
            }
        } else if raw {
            // Raw mode: dump to <name_hash>.<ext_hash>
            let filename = format!("{:016x}.{:016x}", file.name, file.ext);
            let out_path = output.join(&filename);
            fs::write(&out_path, &bytes)
                .with_context(|| format!("writing {}", out_path.display()))?;
        } else if let Some(dict) = dictionary {
            // Dictionary mode: resolve name hash to path
            if let Some(resolved_path) =
                dict.resolve(file.name).and_then(|rp| safe_join(output, rp))
            {
                // Use the resolved path directly, preserving directory structure
                let out_path = resolved_path;
                if let Some(parent) = out_path.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::write(&out_path, &bytes)
                    .with_context(|| format!("writing {}", out_path.display()))?;
            } else {
                // Fall back to hash-based naming
                let dir = output.join(ext_name);
                fs::create_dir_all(&dir)?;
                let filename = format!("{:016x}", file.name);
                let out_path = dir.join(&filename);
                fs::write(&out_path, &bytes)
                    .with_context(|| format!("writing {}", out_path.display()))?;
                unresolved += 1;
            }
        } else {
            // Named mode: dump to <ext>/<name_hash>
            let dir = output.join(ext_name);
            fs::create_dir_all(&dir)?;
            let filename = format!("{:016x}", file.name);
            let out_path = dir.join(&filename);
            fs::write(&out_path, &bytes)
                .with_context(|| format!("writing {}", out_path.display()))?;
        }

        total_bytes += bytes.len() as u64;
        count += 1;
    }

    if let Some(_dict) = dictionary {
        eprintln!(
            "Extracted {} files ({} bytes) to {} ({} unresolved)",
            count,
            total_bytes,
            output.display(),
            unresolved
        );
    } else if lua_chunknames && unnamed_lua > 0 {
        eprintln!(
            "Extracted {} files ({} bytes) to {} ({} unnamed lua)",
            count,
            total_bytes,
            output.display(),
            unnamed_lua
        );
    } else {
        eprintln!(
            "Extracted {} files ({} bytes) to {}",
            count,
            total_bytes,
            output.display()
        );
    }
    Ok(())
}

/// Dump all hashes from the bundle index with resolved extension names.
fn cmd_dump_hashes(input: &Path) -> Result<()> {
    let mut bundle = open_bundle(input)?;
    let index = bundle.read_index()?;

    println!("ext_hash\t\t\tname_hash\t\t\text\tmode");

    for entry in &index {
        let ext_name = lookup_extension(entry.ext).unwrap_or("unknown").to_string();
        println!(
            "{:016x}\t{:016x}\t{}\t{}",
            entry.ext, entry.name, ext_name, entry.mode
        );
    }

    eprintln!("{} entries", index.len());
    Ok(())
}

/// Scan bundle content for path strings and build a dictionary.
fn cmd_scan(inputs: &[PathBuf], output: &Path, merge: Option<&Path>, oodle: &Oodle) -> Result<()> {
    let mut dictionary = if let Some(merge_path) = merge {
        let merge_str = merge_path.to_str().context("Invalid merge path")?;
        eprintln!("Loading existing dictionary from {}", merge_str);
        Dictionary::load(merge_str)?
    } else {
        Dictionary::new()
    };

    let mut total_strings = 0usize;

    for input in inputs {
        eprintln!("Scanning {}...", input.display());
        let mut bundle = open_bundle(input)?;
        let files = bundle.extract_files(oodle)?;

        for file in &files {
            let strings = scan_strings(&file.data);
            total_strings += strings.len();
            for s in strings {
                dictionary.add_path(&s);
            }
        }
    }

    // Save dictionary
    let output_str = output.to_str().context("Invalid output path")?;
    dictionary.save(output_str)?;

    eprintln!(
        "Dictionary saved to {} ({} unique hashes from {} strings)",
        output_str,
        dictionary.len(),
        total_strings
    );
    Ok(())
}

/// Check dictionary coverage against bundle index hashes.
fn cmd_coverage(dictionary_path: &Path, inputs: &[PathBuf]) -> Result<()> {
    let dict_path = dictionary_path
        .to_str()
        .context("Invalid dictionary path")?;
    let dictionary = Dictionary::load(dict_path)?;

    eprintln!("Dictionary loaded: {} entries", dictionary.len());

    // Collect all unique name hashes from all bundles
    let mut all_name_hashes: HashSet<u64> = HashSet::new();
    let mut ext_hash_counts: std::collections::HashMap<u64, usize> =
        std::collections::HashMap::new();

    for input in inputs {
        eprintln!("Reading index from {}...", input.display());
        let mut bundle = open_bundle(input)?;
        let index = bundle.read_index()?;

        for entry in &index {
            all_name_hashes.insert(entry.name);
            *ext_hash_counts.entry(entry.ext).or_insert(0) += 1;
        }
    }

    let total_hashes = all_name_hashes.len();
    let mut covered = 0usize;
    let mut uncovered: Vec<u64> = Vec::new();

    for hash in &all_name_hashes {
        if dictionary.resolve(*hash).is_some() {
            covered += 1;
        } else {
            uncovered.push(*hash);
        }
    }

    let coverage = if total_hashes > 0 {
        (covered as f64 / total_hashes as f64) * 100.0
    } else {
        0.0
    };

    eprintln!();
    eprintln!("Coverage Report:");
    eprintln!("  Total unique name hashes: {}", total_hashes);
    eprintln!("  Covered by dictionary:    {} ({:.1}%)", covered, coverage);
    eprintln!("  Uncovered:                {}", uncovered.len());

    // Show extension breakdown
    eprintln!();
    eprintln!("Extension Breakdown:");
    let mut ext_entries: Vec<(&u64, &usize)> = ext_hash_counts.iter().collect();
    ext_entries.sort_by_key(|(_, count)| std::cmp::Reverse(**count));

    for (ext_hash, count) in &ext_entries {
        let ext_name = lookup_extension(**ext_hash).unwrap_or("unknown");
        eprintln!("  {:016x} ({:15}): {} files", ext_hash, ext_name, count);
    }

    // List uncovered hashes
    if !uncovered.is_empty() {
        eprintln!();
        eprintln!("Uncovered name hashes (first 50):");
        uncovered.sort();
        for hash in &uncovered[..uncovered.len().min(50)] {
            println!("{:016x}", hash);
        }
        if uncovered.len() > 50 {
            eprintln!("... and {} more", uncovered.len() - 50);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn safe_join_rejects_absolute_unix() {
        let output = Path::new("output");
        assert_eq!(safe_join(output, "/etc/evil"), None);
    }

    #[test]
    fn safe_join_rejects_absolute_windows() {
        let output = Path::new("output");
        // On Unix, a string like "C:\Windows\evil" is parsed as a single Normal component
        // containing backslashes. This is platform-dependent behavior, so we only test
        // that it's treated as a relative path and joined.
        // On Windows, Component::Prefix would catch this.
        let result = safe_join(output, "C:\\Windows\\evil");
        #[cfg(windows)]
        assert_eq!(
            result, None,
            "Windows should reject absolute paths with prefixes"
        );
        #[cfg(unix)]
        assert!(result.is_some(), "Unix treats this as a relative path");
    }

    #[test]
    fn safe_join_rejects_parent_dir() {
        let output = Path::new("output");
        assert_eq!(safe_join(output, "../escape"), None);
        assert_eq!(safe_join(output, "a/../../b"), None);
    }

    #[test]
    fn safe_join_accepts_normal_relative() {
        let output = Path::new("output");
        assert_eq!(
            safe_join(output, "scripts/foo/bar.lua"),
            Some(PathBuf::from("output/scripts/foo/bar.lua"))
        );
    }
}
