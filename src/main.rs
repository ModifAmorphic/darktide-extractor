use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::BufWriter;
use std::io::Write as IoWrite;
use std::path::{Component, Path, PathBuf};

mod discovery;

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
            Component::RootDir | Component::ParentDir | Component::Prefix(_) | Component::CurDir
        )
    }) {
        return None;
    }
    Some(output.join(p))
}

/// Configuration for file writing operations.
struct WriteConfig<'a> {
    raw: bool,
    lua_chunknames: bool,
    dictionary: Option<&'a Dictionary>,
}

/// Result of writing a file entry.
struct WrittenFile {
    path: PathBuf,
    fell_back: bool,
    unnamed: bool,
}

/// State for tracking collisions during extraction.
struct CollisionState {
    written_paths: HashSet<PathBuf>,
    collisions: Vec<(PathBuf, Vec<PathBuf>)>,
}

/// Write a single file entry to disk.
/// Returns the output path if written, or None if skipped (e.g., due to collision mode).
fn write_file_entry<'a>(
    file: &darktide_bundle::FileEntry,
    output: &Path,
    config: &WriteConfig<'a>,
    state: &mut CollisionState,
    source_bundle: &Path,
    collision_mode: CollisionMode,
) -> Result<Option<WrittenFile>> {
    let ext_name = lookup_extension(file.ext).unwrap_or("unknown");

    // Lua files are unconditionally normalized to standard LuaJIT bytecode
    let bytes = if ext_name == "lua" {
        normalize_luajit(&file.data)
    } else {
        std::borrow::Cow::Borrowed(file.data.as_slice())
    };

    let (out_path, fell_back, unnamed) = if config.lua_chunknames && ext_name == "lua" {
        if let Some(chunkname) = extract_chunkname(&file.data).and_then(|cn| safe_join(output, &cn))
        {
            (chunkname, false, false)
        } else {
            // No chunkname found or unsafe path — place in unnamed/ with hash-based name
            let dir = output.join("unnamed");
            let filename = format!("{:016x}", file.name);
            (dir.join(&filename), false, true)
        }
    } else if config.raw {
        // Raw mode: dump to <name_hash>.<ext_hash>
        let filename = format!("{:016x}.{:016x}", file.name, file.ext);
        (output.join(&filename), false, false)
    } else if let Some(dict) = config.dictionary {
        // Dictionary mode: resolve name hash to path
        if let Some(resolved_path) = dict.resolve(file.name).and_then(|rp| safe_join(output, rp)) {
            (resolved_path, false, false)
        } else {
            // Fall back to hash-based naming
            let dir = output.join(ext_name);
            let filename = format!("{:016x}", file.name);
            (dir.join(&filename), true, false)
        }
    } else {
        // Named mode: dump to <ext>/<name_hash>
        let dir = output.join(ext_name);
        let filename = format!("{:016x}", file.name);
        (dir.join(&filename), false, false)
    };

    // Check for collisions
    if state.written_paths.contains(&out_path) {
        // Find or create collision entry
        let collision_entry = state
            .collisions
            .iter_mut()
            .find(|(path, _)| path == &out_path)
            .unwrap();

        match collision_mode {
            CollisionMode::Overwrite => {
                // Rewrite the file and record the collision
                if let Some(parent) = out_path.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::write(&out_path, &bytes)
                    .with_context(|| format!("writing {}", out_path.display()))?;
                collision_entry.1.push(source_bundle.to_path_buf());
                Ok(Some(WrittenFile {
                    path: out_path,
                    fell_back,
                    unnamed,
                }))
            }
            CollisionMode::Skip => {
                // Do not write, just record the collision
                collision_entry.1.push(source_bundle.to_path_buf());
                Ok(None)
            }
            CollisionMode::Error => {
                collision_entry.1.push(source_bundle.to_path_buf());
                Err(anyhow::anyhow!(
                    "Collision: multiple files would write to {}",
                    out_path.display()
                ))
            }
        }
    } else {
        // No collision, write the file
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&out_path, &bytes).with_context(|| format!("writing {}", out_path.display()))?;
        state.written_paths.insert(out_path.clone());
        state
            .collisions
            .push((out_path.clone(), vec![source_bundle.to_path_buf()]));
        Ok(Some(WrittenFile {
            path: out_path,
            fell_back,
            unnamed,
        }))
    }
}

#[derive(Parser)]
#[command(name = "dtex")]
#[command(about = "Extract files from Darktide resource bundles")]
#[command(version)]
struct Cli {
    /// Path to the Oodle shared library
    #[arg(long, global = true)]
    oodle_lib: Option<String>,

    /// Darktide game directory (for Steam auto-discovery and Windows Oodle DLL)
    #[arg(long, global = true)]
    game_dir: Option<PathBuf>,

    /// Enable verbose output (including Oodle debug output)
    #[arg(long, global = true)]
    verbose: bool,

    /// Suppress stderr progress/summary (errors still print)
    #[arg(long, global = true)]
    quiet: bool,

    /// Output machine-readable JSON for data commands
    #[arg(long, global = true)]
    json: bool,

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
        /// Path to bundle file or directory (optional: uses --game-dir if not given)
        #[arg(short, long)]
        input: Option<PathBuf>,

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

        /// How to handle output path collisions
        #[arg(long, default_value = "overwrite")]
        on_collision: CollisionMode,

        /// Abort on first error (instead of continuing)
        #[arg(long)]
        strict: bool,

        /// Write manifest TSV of extracted files
        #[arg(long)]
        manifest: Option<PathBuf>,
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

    /// Find files by extension in bundles (no decompression)
    Find {
        /// Filter by extension (e.g. "lua", "texture")
        extension: Option<String>,

        /// Path to bundle file or directory (optional: uses --game-dir if not given)
        #[arg(short, long)]
        input: Option<PathBuf>,
    },

    /// Validate bundle files in a directory
    Validate {
        /// Path to bundle file or directory (optional: uses --game-dir if not given)
        #[arg(short, long)]
        input: Option<PathBuf>,
    },
}

#[derive(Clone, Copy, ValueEnum, PartialEq, Eq, Debug)]
enum CollisionMode {
    Overwrite,
    Skip,
    Error,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Resolve game directory early (needed for Oodle discovery)
    let game_dir = discovery::resolve_game_dir(cli.game_dir.as_deref());

    let exit_code = match cli.command {
        Commands::List { input, extension } => {
            if cmd_list(&input, extension.as_deref(), cli.json, cli.quiet)? {
                0
            } else {
                1
            }
        }
        Commands::Extract {
            input,
            output,
            extension,
            raw,
            dictionary,
            lua_chunknames,
            on_collision,
            strict,
            manifest,
        } => {
            let oodle = discovery::resolve_oodle(
                cli.oodle_lib.as_deref(),
                game_dir.as_deref(),
                cli.verbose,
            )
            .context("Failed to load Oodle library")?;

            let dict = if let Some(dict_path) = dictionary {
                Some(Dictionary::load(
                    dict_path.to_str().context("Invalid dictionary path")?,
                )?)
            } else {
                None
            };

            let input_kind = discovery::resolve_input(input.as_deref(), game_dir.as_deref())?;
            if cmd_extract(
                &input_kind,
                &output,
                extension.as_deref(),
                raw,
                lua_chunknames,
                on_collision,
                strict,
                manifest.as_deref(),
                cli.json,
                cli.quiet,
                &oodle,
                dict.as_ref(),
            )? {
                0
            } else {
                1
            }
        }
        Commands::DumpHashes { input } => {
            if cmd_dump_hashes(&input, cli.json, cli.quiet)? {
                0
            } else {
                1
            }
        }
        Commands::Scan {
            input,
            output,
            merge,
        } => {
            let oodle = discovery::resolve_oodle(
                cli.oodle_lib.as_deref(),
                game_dir.as_deref(),
                cli.verbose,
            )
            .context("Failed to load Oodle library")?;
            if cmd_scan(&input, &output, merge.as_deref(), &oodle, cli.quiet)? {
                0
            } else {
                1
            }
        }
        Commands::Coverage { dictionary, input } => {
            if cmd_coverage(&dictionary, &input, cli.json, cli.quiet)? {
                0
            } else {
                1
            }
        }
        Commands::Find { extension, input } => {
            let input_kind = discovery::resolve_input(input.as_deref(), game_dir.as_deref())?;
            if cmd_find(&input_kind, extension.as_deref(), cli.json, cli.quiet)? {
                0
            } else {
                1
            }
        }
        Commands::Validate { input } => {
            let input_kind = discovery::resolve_input(input.as_deref(), game_dir.as_deref())?;
            if cmd_validate(&input_kind, cli.json, cli.quiet)? {
                0
            } else {
                1
            }
        }
    };

    std::process::exit(exit_code);
}

fn open_bundle(path: &Path) -> Result<Bundle> {
    Bundle::open(path.to_str().context("Invalid bundle path")?)
        .with_context(|| format!("Failed to open bundle: {}", path.display()))
}

/// List files in a bundle (index only, no decompression needed).
fn cmd_list(input: &Path, extension_filter: Option<&str>, json: bool, quiet: bool) -> Result<bool> {
    let mut bundle = open_bundle(input)?;
    let index = bundle.read_index()?;

    let ext_hash_filter = extension_filter.map(|e| murmur_hash64(e.as_bytes()));

    let mut entries = Vec::new();

    for entry in &index {
        if let Some(hash) = ext_hash_filter {
            if entry.ext != hash {
                continue;
            }
        }

        let ext_name = lookup_extension(entry.ext).unwrap_or("unknown").to_string();

        if json {
            entries.push(serde_json::json!({
                "ext_hash": format!("{:016x}", entry.ext),
                "name_hash": format!("{:016x}", entry.name),
                "ext": ext_name,
                "mode": entry.mode,
            }));
        } else {
            println!(
                "{:016x}\t{:016x}\t{}\tmode={}",
                entry.ext, entry.name, ext_name, entry.mode
            );
        }
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&entries)?);
    } else if !quiet {
        eprintln!("{} files listed", entries.len());
    }

    Ok(true)
}

/// Extract files from a bundle or directory of bundles.
#[allow(clippy::too_many_arguments)]
fn cmd_extract(
    input_kind: &discovery::InputKind,
    output: &Path,
    extension_filter: Option<&str>,
    raw: bool,
    lua_chunknames: bool,
    collision_mode: CollisionMode,
    strict: bool,
    manifest_path: Option<&Path>,
    json: bool,
    quiet: bool,
    oodle: &Oodle,
    dictionary: Option<&Dictionary>,
) -> Result<bool> {
    let ext_hash_filter = extension_filter.map(|e| murmur_hash64(e.as_bytes()));
    let config = WriteConfig {
        raw,
        lua_chunknames,
        dictionary,
    };

    match input_kind {
        discovery::InputKind::SingleBundle(bundle_path) => {
            // Single bundle extraction (original behavior)
            let mut bundle = open_bundle(bundle_path)?;
            let files = bundle.extract_files(oodle)?;

            fs::create_dir_all(output)?;

            let mut count = 0u64;
            let mut total_bytes = 0u64;
            let mut unresolved = 0u64;
            let mut unnamed_lua = 0u64;
            let mut manifest_entries = Vec::new();
            let mut state = CollisionState {
                written_paths: HashSet::new(),
                collisions: Vec::new(),
            };

            for file in &files {
                if let Some(hash) = ext_hash_filter {
                    if file.ext != hash {
                        continue;
                    }
                }

                if let Some(written) = write_file_entry(
                    file,
                    output,
                    &config,
                    &mut state,
                    bundle_path,
                    collision_mode,
                )? {
                    total_bytes += file.data.len() as u64;
                    count += 1;

                    if manifest_path.is_some() {
                        let ext_name = lookup_extension(file.ext).unwrap_or("unknown");
                        manifest_entries.push((
                            written.path.display().to_string(),
                            bundle_path.display().to_string(),
                            format!("{:016x}", file.name),
                            ext_name.to_string(),
                        ));
                    }

                    // Track unresolved and unnamed counts based on what actually happened
                    if written.fell_back {
                        unresolved += 1;
                    }
                    if written.unnamed {
                        unnamed_lua += 1;
                    }
                }
            }

            // Write manifest if requested
            if let Some(manifest) = manifest_path {
                write_manifest(manifest, &manifest_entries)?;
            }

            // Output summary
            if json {
                let summary = serde_json::json!({
                    "files_extracted": count,
                    "bytes": total_bytes,
                    "unresolved": unresolved,
                    "unnamed_lua": unnamed_lua,
                    "bundle": bundle_path.display().to_string(),
                });
                println!("{}", serde_json::to_string_pretty(&summary)?);
            } else if !quiet {
                if dictionary.is_some() {
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
            }

            Ok(true)
        }
        discovery::InputKind::BundleDir(dir_path) => {
            // Directory mode: iterate over all bundles
            fs::create_dir_all(output)?;

            let mut bundles_processed = 0usize;
            let mut bundles_skipped_nonbundle = 0usize;
            let mut files_extracted = 0u64;
            let mut total_bytes = 0u64;
            let mut errors = 0u64;
            let mut manifest_entries = Vec::new();

            // Track written paths and collisions
            let mut state = CollisionState {
                written_paths: HashSet::new(),
                collisions: Vec::new(),
            };

            // Get all top-level files
            let entries: Vec<_> = fs::read_dir(dir_path)?
                .filter_map(|e| e.ok())
                .filter(|e| e.path().is_file())
                .collect();

            let total_files = entries.len();

            for (idx, entry) in entries.iter().enumerate() {
                let bundle_path = entry.path();

                // Progress output
                if !quiet {
                    eprintln!("[{}/{}] {}", idx + 1, total_files, bundle_path.display());
                }

                // Classify the file
                let path_str = bundle_path
                    .to_str()
                    .context("Invalid UTF-8 in bundle path")?;
                let file_class = Bundle::classify(path_str);

                match file_class {
                    darktide_bundle::FileClass::Bundle => {
                        bundles_processed += 1;

                        // Open and extract
                        match extract_bundle_to_dir(
                            &bundle_path,
                            output,
                            ext_hash_filter,
                            &config,
                            &mut state,
                            &mut manifest_entries,
                            oodle,
                            collision_mode,
                        ) {
                            Ok((count, bytes)) => {
                                files_extracted += count;
                                total_bytes += bytes;
                            }
                            Err(e) => {
                                errors += 1;
                                eprintln!("Error processing {}: {}", bundle_path.display(), e);
                                if strict {
                                    return Ok(false);
                                }
                            }
                        }
                    }
                    darktide_bundle::FileClass::NotBundle => {
                        bundles_skipped_nonbundle += 1;
                    }
                    darktide_bundle::FileClass::Unreadable => {
                        bundles_skipped_nonbundle += 1;
                    }
                }
            }

            // Write manifest if requested
            if let Some(manifest) = manifest_path {
                write_manifest(manifest, &manifest_entries)?;
            }

            // Count actual collisions (more than one source bundle)
            let actual_collisions = state
                .collisions
                .iter()
                .filter(|(_, sources)| sources.len() > 1)
                .count();

            // Output summary
            if json {
                let summary = serde_json::json!({
                    "bundles_processed": bundles_processed,
                    "bundles_skipped_nonbundle": bundles_skipped_nonbundle,
                    "files_extracted": files_extracted,
                    "bytes": total_bytes,
                    "collisions": actual_collisions,
                    "errors": errors,
                });
                println!("{}", serde_json::to_string_pretty(&summary)?);
            } else if !quiet {
                eprintln!();
                eprintln!("Summary:");
                eprintln!("  Bundles processed: {}", bundles_processed);
                eprintln!("  Skipped (non-bundle): {}", bundles_skipped_nonbundle);
                eprintln!("  Files extracted: {}", files_extracted);
                eprintln!("  Total bytes: {}", total_bytes);
                eprintln!("  Collisions: {}", actual_collisions);
                eprintln!("  Errors: {}", errors);
            }

            // Print collision report if any (always to stderr unless --quiet)
            if actual_collisions > 0 && !quiet {
                eprintln!();
                eprintln!("Collisions:");
                for (out_path, sources) in &state.collisions {
                    if sources.len() > 1 {
                        eprintln!("  {}:", out_path.display());
                        for source in sources {
                            eprintln!("    - {}", source.display());
                        }
                    }
                }
            }

            Ok(errors == 0)
        }
    }
}

/// Extract files from a single bundle to a directory, updating shared state.
#[allow(clippy::too_many_arguments)]
fn extract_bundle_to_dir<'a>(
    bundle_path: &Path,
    output: &Path,
    ext_hash_filter: Option<u64>,
    config: &WriteConfig<'a>,
    state: &mut CollisionState,
    manifest_entries: &mut Vec<(String, String, String, String)>,
    oodle: &Oodle,
    collision_mode: CollisionMode,
) -> Result<(u64, u64)> {
    let mut bundle = open_bundle(bundle_path)?;
    let files = bundle.extract_files(oodle)?;

    let mut count = 0u64;
    let mut bytes = 0u64;

    for file in &files {
        if let Some(hash) = ext_hash_filter {
            if file.ext != hash {
                continue;
            }
        }

        if let Some(written) =
            write_file_entry(file, output, config, state, bundle_path, collision_mode)?
        {
            bytes += file.data.len() as u64;
            count += 1;

            let ext_name = lookup_extension(file.ext).unwrap_or("unknown");
            manifest_entries.push((
                written.path.display().to_string(),
                bundle_path.display().to_string(),
                format!("{:016x}", file.name),
                ext_name.to_string(),
            ));
        }
    }

    Ok((count, bytes))
}

/// Write manifest TSV file.
fn write_manifest(path: &Path, entries: &[(String, String, String, String)]) -> Result<()> {
    let file = fs::File::create(path)?;
    let mut writer = BufWriter::new(file);

    for (output_path, source_bundle, name_hash, ext) in entries {
        writeln!(
            writer,
            "{}\t{}\t{}\t{}",
            output_path, source_bundle, name_hash, ext
        )?;
    }

    writer.flush()?;
    Ok(())
}

/// Dump all hashes from the bundle index with resolved extension names.
fn cmd_dump_hashes(input: &Path, json: bool, quiet: bool) -> Result<bool> {
    let mut bundle = open_bundle(input)?;
    let index = bundle.read_index()?;

    if json {
        let entries: Vec<serde_json::Value> = index
            .iter()
            .map(|entry| {
                let ext_name = lookup_extension(entry.ext).unwrap_or("unknown").to_string();
                serde_json::json!({
                    "ext_hash": format!("{:016x}", entry.ext),
                    "name_hash": format!("{:016x}", entry.name),
                    "ext": ext_name,
                    "mode": entry.mode,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&entries)?);
    } else {
        println!("ext_hash\t\t\tname_hash\t\t\text\tmode");

        for entry in &index {
            let ext_name = lookup_extension(entry.ext).unwrap_or("unknown").to_string();
            println!(
                "{:016x}\t{:016x}\t{}\t{}",
                entry.ext, entry.name, ext_name, entry.mode
            );
        }
    }

    if !quiet {
        eprintln!("{} entries", index.len());
    }

    Ok(true)
}

/// Scan bundle content for path strings and build a dictionary.
fn cmd_scan(
    inputs: &[PathBuf],
    output: &Path,
    merge: Option<&Path>,
    oodle: &Oodle,
    quiet: bool,
) -> Result<bool> {
    let mut dictionary = if let Some(merge_path) = merge {
        let merge_str = merge_path.to_str().context("Invalid merge path")?;
        if !quiet {
            eprintln!("Loading existing dictionary from {}", merge_str);
        }
        Dictionary::load(merge_str)?
    } else {
        Dictionary::new()
    };

    let mut total_strings = 0usize;

    for input in inputs {
        if !quiet {
            eprintln!("Scanning {}...", input.display());
        }
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

    if !quiet {
        eprintln!(
            "Dictionary saved to {} ({} unique hashes from {} strings)",
            output_str,
            dictionary.len(),
            total_strings
        );
    }
    Ok(true)
}

/// Check dictionary coverage against bundle index hashes.
fn cmd_coverage(
    dictionary_path: &Path,
    inputs: &[PathBuf],
    json: bool,
    quiet: bool,
) -> Result<bool> {
    let dict_path = dictionary_path
        .to_str()
        .context("Invalid dictionary path")?;
    let dictionary = Dictionary::load(dict_path)?;

    if !quiet {
        eprintln!("Dictionary loaded: {} entries", dictionary.len());
    }

    // Collect all unique name hashes from all bundles
    let mut all_name_hashes: HashSet<u64> = HashSet::new();
    let mut ext_hash_counts: HashMap<u64, usize> = HashMap::new();

    for input in inputs {
        if !quiet {
            eprintln!("Reading index from {}...", input.display());
        }
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

    // Build extension breakdown
    let mut ext_breakdown: Vec<(u64, &str, usize)> = ext_hash_counts
        .iter()
        .map(|(ext_hash, count)| {
            let ext_name = lookup_extension(*ext_hash).unwrap_or("unknown");
            (*ext_hash, ext_name, *count)
        })
        .collect();
    ext_breakdown.sort_by_key(|(_, _, count)| std::cmp::Reverse(*count));

    if json {
        let summary = serde_json::json!({
            "total_unique_name_hashes": total_hashes,
            "covered": covered,
            "uncovered": uncovered.len(),
            "coverage_pct": coverage,
            "extension_breakdown": ext_breakdown
                .into_iter()
                .map(|(ext_hash, ext_name, count)| serde_json::json!({
                    "ext_hash": format!("{:016x}", ext_hash),
                    "ext": ext_name,
                    "count": count,
                }))
                .collect::<Vec<_>>(),
            "uncovered_hashes": uncovered
                .into_iter()
                .map(|h| format!("{:016x}", h))
                .collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&summary)?);
    } else {
        eprintln!();
        eprintln!("Coverage Report:");
        eprintln!("  Total unique name hashes: {}", total_hashes);
        eprintln!("  Covered by dictionary:    {} ({:.1}%)", covered, coverage);
        eprintln!("  Uncovered:                {}", uncovered.len());

        // Show extension breakdown
        eprintln!();
        eprintln!("Extension Breakdown:");
        for (ext_hash, ext_name, count) in &ext_breakdown {
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
    }

    Ok(true)
}

/// Find files by extension in bundles (no decompression).
fn cmd_find(
    input_kind: &discovery::InputKind,
    extension_filter: Option<&str>,
    json: bool,
    quiet: bool,
) -> Result<bool> {
    let ext_hash_filter = extension_filter.map(|e| murmur_hash64(e.as_bytes()));

    match input_kind {
        discovery::InputKind::SingleBundle(bundle_path) => {
            find_in_bundle(bundle_path, ext_hash_filter, json, quiet)
        }
        discovery::InputKind::BundleDir(dir_path) => {
            find_in_dir(dir_path, ext_hash_filter, json, quiet)
        }
    }
}

/// Find files in a single bundle.
fn find_in_bundle(
    bundle_path: &Path,
    ext_hash_filter: Option<u64>,
    json: bool,
    quiet: bool,
) -> Result<bool> {
    let mut bundle = open_bundle(bundle_path)?;
    let index = bundle.read_index()?;

    let mut matches = Vec::new();
    let mut ext_counts: HashMap<&str, usize> = HashMap::new();

    for entry in &index {
        let ext_name = lookup_extension(entry.ext).unwrap_or("unknown");

        if let Some(hash) = ext_hash_filter {
            if entry.ext == hash {
                if json {
                    matches.push(serde_json::json!({
                        "bundle": bundle_path.display().to_string(),
                        "name_hash": format!("{:016x}", entry.name),
                        "ext": ext_name,
                        "mode": entry.mode,
                    }));
                } else {
                    println!(
                        "{}\t{:016x}\t{}\t{}",
                        bundle_path.display(),
                        entry.name,
                        ext_name,
                        entry.mode
                    );
                }
            }
        } else {
            *ext_counts.entry(ext_name).or_insert(0) += 1;
        }
    }

    if ext_hash_filter.is_none() {
        // Summary mode
        if json {
            let mut summary: Vec<serde_json::Value> = ext_counts
                .into_iter()
                .map(|(ext, count)| serde_json::json!({ "ext": ext, "count": count }))
                .collect();
            summary.sort_by_key(|v| v["count"].as_u64().unwrap());
            summary.reverse();
            println!("{}", serde_json::to_string_pretty(&summary)?);
        } else {
            let mut ext_entries: Vec<_> = ext_counts.into_iter().collect();
            ext_entries.sort_by_key(|(_, count)| std::cmp::Reverse(*count));

            for (ext_name, count) in &ext_entries {
                println!("{}\t{}", ext_name, count);
            }
            if !quiet {
                eprintln!("Total bundles scanned: 1");
            }
        }
    } else {
        // Filter mode
        if json {
            println!("{}", serde_json::to_string_pretty(&matches)?);
        } else if !quiet {
            eprintln!("{} matches found", matches.len());
        }
    }

    Ok(true)
}

/// Find files in a directory of bundles.
fn find_in_dir(
    dir_path: &Path,
    ext_hash_filter: Option<u64>,
    json: bool,
    quiet: bool,
) -> Result<bool> {
    let entries: Vec<_> = fs::read_dir(dir_path)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file())
        .collect();

    let mut matches = Vec::new();
    let mut ext_counts: HashMap<&str, usize> = HashMap::new();
    let mut bundles_scanned = 0usize;

    for entry in &entries {
        let bundle_path = entry.path();

        let path_str = bundle_path
            .to_str()
            .context("Invalid UTF-8 in bundle path")?;
        let file_class = Bundle::classify(path_str);

        if file_class != darktide_bundle::FileClass::Bundle {
            continue;
        }

        bundles_scanned += 1;

        let mut bundle = match open_bundle(&bundle_path) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("Error opening {}: {}", bundle_path.display(), e);
                continue;
            }
        };

        let index = match bundle.read_index() {
            Ok(i) => i,
            Err(e) => {
                eprintln!("Error reading index {}: {}", bundle_path.display(), e);
                continue;
            }
        };

        for entry in &index {
            let ext_name = lookup_extension(entry.ext).unwrap_or("unknown");

            if let Some(hash) = ext_hash_filter {
                if entry.ext == hash {
                    if json {
                        matches.push(serde_json::json!({
                            "bundle": bundle_path.display().to_string(),
                            "name_hash": format!("{:016x}", entry.name),
                            "ext": ext_name,
                            "mode": entry.mode,
                        }));
                    } else {
                        println!(
                            "{}\t{:016x}\t{}\t{}",
                            bundle_path.display(),
                            entry.name,
                            ext_name,
                            entry.mode
                        );
                    }
                }
            } else {
                *ext_counts.entry(ext_name).or_insert(0) += 1;
            }
        }
    }

    if ext_hash_filter.is_none() {
        // Summary mode
        if json {
            let mut summary: Vec<serde_json::Value> = ext_counts
                .into_iter()
                .map(|(ext, count)| serde_json::json!({ "ext": ext, "count": count }))
                .collect();
            summary.sort_by_key(|v| v["count"].as_u64().unwrap());
            summary.reverse();
            println!("{}", serde_json::to_string_pretty(&summary)?);
        } else {
            let mut ext_entries: Vec<_> = ext_counts.into_iter().collect();
            ext_entries.sort_by_key(|(_, count)| std::cmp::Reverse(*count));

            for (ext_name, count) in &ext_entries {
                println!("{}\t{}", ext_name, count);
            }
            if !quiet {
                eprintln!("Total bundles scanned: {}", bundles_scanned);
            }
        }
    } else {
        // Filter mode
        if json {
            println!("{}", serde_json::to_string_pretty(&matches)?);
        } else if !quiet {
            eprintln!("{} matches found", matches.len());
        }
    }

    Ok(true)
}

/// Validate bundle files in a directory.
fn cmd_validate(input_kind: &discovery::InputKind, json: bool, quiet: bool) -> Result<bool> {
    match input_kind {
        discovery::InputKind::SingleBundle(bundle_path) => {
            validate_single(bundle_path, json, quiet)
        }
        discovery::InputKind::BundleDir(dir_path) => validate_dir(dir_path, json, quiet),
    }
}

/// Validate a single bundle file.
fn validate_single(bundle_path: &Path, json: bool, quiet: bool) -> Result<bool> {
    let path_str = bundle_path
        .to_str()
        .context("Invalid UTF-8 in bundle path")?;
    let file_class = Bundle::classify(path_str);

    if json {
        let result = serde_json::json!({
            "bundle": vec![bundle_path.display().to_string()],
            "not_bundle": Vec::<String>::new(),
            "unreadable": Vec::<String>::new(),
            "total": 1,
            "samples": {
                "bundle": vec![bundle_path.display().to_string()],
                "not_bundle": Vec::<String>::new(),
                "unreadable": Vec::<String>::new(),
            }
        });

        match file_class {
            darktide_bundle::FileClass::Bundle => {
                println!("{}", serde_json::to_string_pretty(&result)?);
            }
            darktide_bundle::FileClass::NotBundle => {
                let result = serde_json::json!({
                    "bundle": Vec::<String>::new(),
                    "not_bundle": vec![bundle_path.display().to_string()],
                    "unreadable": Vec::<String>::new(),
                    "total": 1,
                    "samples": {
                        "bundle": Vec::<String>::new(),
                        "not_bundle": vec![bundle_path.display().to_string()],
                        "unreadable": Vec::<String>::new(),
                    }
                });
                println!("{}", serde_json::to_string_pretty(&result)?);
            }
            darktide_bundle::FileClass::Unreadable => {
                let result = serde_json::json!({
                    "bundle": Vec::<String>::new(),
                    "not_bundle": Vec::<String>::new(),
                    "unreadable": vec![bundle_path.display().to_string()],
                    "total": 1,
                    "samples": {
                        "bundle": Vec::<String>::new(),
                        "not_bundle": Vec::<String>::new(),
                        "unreadable": vec![bundle_path.display().to_string()],
                    }
                });
                println!("{}", serde_json::to_string_pretty(&result)?);
            }
        }
    } else {
        match file_class {
            darktide_bundle::FileClass::Bundle => {
                println!("Bundle: {}", bundle_path.display());
            }
            darktide_bundle::FileClass::NotBundle => {
                println!("NotBundle: {}", bundle_path.display());
            }
            darktide_bundle::FileClass::Unreadable => {
                println!("Unreadable: {}", bundle_path.display());
            }
        }
    }

    if !quiet {
        eprintln!("Total: 1 file");
    }

    Ok(true)
}

/// Validate all files in a directory.
fn validate_dir(dir_path: &Path, json: bool, quiet: bool) -> Result<bool> {
    let entries: Vec<_> = fs::read_dir(dir_path)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file())
        .collect();

    let mut bundles = Vec::new();
    let mut not_bundles = Vec::new();
    let mut unreadable = Vec::new();

    for entry in &entries {
        let path = entry.path();
        let path_str = path.to_str().context("Invalid UTF-8 in path")?;
        let file_class = Bundle::classify(path_str);

        match file_class {
            darktide_bundle::FileClass::Bundle => {
                bundles.push(path.display().to_string());
            }
            darktide_bundle::FileClass::NotBundle => {
                not_bundles.push(path.display().to_string());
            }
            darktide_bundle::FileClass::Unreadable => {
                unreadable.push(path.display().to_string());
            }
        }
    }

    let total = bundles.len() + not_bundles.len() + unreadable.len();

    if json {
        let result = serde_json::json!({
            "bundle": bundles.clone(),
            "not_bundle": not_bundles.clone(),
            "unreadable": unreadable.clone(),
            "total": total,
            "samples": {
                "bundle": bundles.into_iter().take(10).collect::<Vec<_>>(),
                "not_bundle": not_bundles.into_iter().take(10).collect::<Vec<_>>(),
                "unreadable": unreadable.into_iter().take(10).collect::<Vec<_>>(),
            }
        });
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        println!("Bundle: {} files", bundles.len());
        for path in bundles.iter().take(10) {
            println!("  {}", path);
        }
        if bundles.len() > 10 {
            println!("  ... and {} more", bundles.len() - 10);
        }

        println!();
        println!("NotBundle: {} files", not_bundles.len());
        for path in not_bundles.iter().take(10) {
            println!("  {}", path);
        }
        if not_bundles.len() > 10 {
            println!("  ... and {} more", not_bundles.len() - 10);
        }

        println!();
        println!("Unreadable: {} files", unreadable.len());
        for path in unreadable.iter().take(10) {
            println!("  {}", path);
        }
        if unreadable.len() > 10 {
            println!("  ... and {} more", unreadable.len() - 10);
        }
    }

    if !quiet {
        eprintln!("Total: {} files", total);
    }

    Ok(true)
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
