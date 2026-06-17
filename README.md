# darktide-extractor

Tools for working with Darktide resource bundles. Provides a Rust library for parsing and
extracting `.bundle` files, a CLI (`dtex`) for interactive use, and a C-compatible FFI
library for integration into other projects. Supports Linux and Windows.

Darktide bundles are Oodle (Kraken) compressed archives that store file names and extensions
as MurmurHash64A hashes rather than plaintext. `dtex` handles decompression, hash resolution,
and file extraction.

## What can be extracted

Every file in a bundle is extractable. Bundles contain dozens of asset types (about 50 known
extensions), including `lua`, `texture`, `package`, `bones`, `particles`, `material`,
`shader`, `wwise_bank`, `wwise_stream`, and more.

How files are **named** on extraction depends on the type:

- **Lua bytecode** — recovered exactly from the bytecode debug info (chunkname) via
  `--lua-chunknames`. This is the only file type whose original path is fully recoverable.
- **Other types** — named by resolving the `name_hash` through a dictionary built with
  `scan`, or fall back to the hex hash under the extension directory.
- **All types** — `--raw` writes files as `<name_hash>.<ext_hash>` with no resolution.

### Limitations

- **`name_hash` is content-addressed**, not derived from the file path. Dictionary-based
  resolution has 0% coverage for `name_hash` fields, so non-Lua files generally cannot be
  recovered to their original paths by scanning bundle content. The dictionary workflow
  remains useful for other hash lookups.
- **Plaintext Lua source** (a small number of entries) is not compiled bytecode; it is passed
  through unchanged and standard LuaJIT tooling skips it.
- **Decompression requires the proprietary Oodle library** at runtime (see Installation).
- **Bundle classification is based on magic bytes**: Only the first 8 bytes are checked,
  so non-bundle files that coincidentally match the magic will be opened and fail during parsing.
- **Directory extraction only processes top-level files**: Subdirectories are not recursed.
  This matches the Darktide bundle directory layout where all bundles are flat and non-bundle
  files are either `.stream` files or in `data/` subdirectories.

See [`docs/bundle-format.md`](docs/bundle-format.md) for the binary format details and
[`docs/luajit-bytecode-format.md`](docs/luajit-bytecode-format.md) for Lua-specific handling.

## Installation

### Prerequisites: the Oodle library

`dtex` dynamically loads the Oodle shared library at runtime. The library is searched in the following order:

1. `--oodle-lib` command-line flag
2. `DTEX_OODLE_LIB` environment variable
3. Windows only: `<game-dir>/binaries/oo2core_9_win64.dll` (if `--game-dir` is provided or auto-discovered)
4. Next to the `dtex` binary (exe-dir)
5. Current working directory
6. System library search path

Place the platform-appropriate file in one of these locations (or use `--oodle-lib`):

- Linux: `liboo2corelinux64.so.9` (Oodle 2.9.14)
- Windows: `oo2core_9_win64.dll` (Oodle 2.9.10)

See [`docs/oodle-library.md`](docs/oodle-library.md) for how to obtain the library from
Epic's CDN.

### Option 1: Pre-built binary

Download the latest release for your platform from the
[Releases](https://github.com/ModifAmorphic/darktide-extractor/releases) page. Unzip it, drop
the Oodle library next to the `dtex` binary, then make `dtex` available on your `PATH`.

**Linux / macOS:**
```sh
mkdir -p ~/.local/bin
cp dtex ~/.local/bin/
cp liboo2corelinux64.so.9 ~/.local/bin/
# Ensure ~/.local/bin is on your PATH (add to ~/.bashrc / ~/.zshrc if not):
export PATH="$HOME/.local/bin:$PATH"
dtex --version
```

**Windows (PowerShell):**
```powershell
$dest = "$env:LOCALAPPDATA\dtex"
New-Item -ItemType Directory -Force -Path $dest | Out-Null
Copy-Item dtex.exe, oo2core_9_win64.dll $dest
# Add to PATH for the current user (run once):
[Environment]::SetEnvironmentVariable("Path", "$dest;" + [Environment]::GetEnvironmentVariable("Path", "User"), "User")
dtex --version
```

If the Oodle library ships with the Darktide game in your Steam installation, you can use
`--game-dir <path>` to auto-discover the DLL on Windows (e.g., `--game-dir "C:\Games\Darktide"`).

### Option 2: Build from source

Requires the [Rust toolchain](https://www.rust-lang.org/tools/install).

```sh
cargo build --release
```

The binary is at `target/release/dtex` (Linux) or `target/release/dtex.exe` (Windows). Copy
it (and the Oodle library) onto your `PATH` as in Option 1.

## CLI usage

### List files in a bundle (no decompression)

```sh
dtex list -i <bundle_file> [extension]
```

### Extract files

```sh
# All files from a single bundle
dtex extract -i <bundle_file> -o <output_dir>

# Filter by extension
dtex extract -i <bundle_file> -o <output_dir> lua

# Extract all bundles from a directory (e.g., the game's bundle/ folder)
dtex extract -i <bundle_dir> -o <output_dir>

# Lua files named by their chunkname (original source paths)
dtex extract -i <bundle_dir> -o <output_dir> --lua-chunknames lua

# Raw mode: <name_hash>.<ext_hash>, no resolution
dtex extract -i <bundle_file> -o <output_dir> --raw

# Dictionary-based name resolution
dtex extract -i <bundle_file> -o <output_dir> --dictionary dictionary.txt
```

#### Directory extraction

When extracting from a directory (e.g., the game's `bundle/` folder), `dtex` processes
all top-level bundle files. Non-bundle files (like `.stream` files and `data/` subdirectories)
are skipped automatically.

```sh
dtex extract -i /path/to/darktide/bundle -o extracted_files
```

#### Collision handling

When multiple bundles contain files that would write to the same output path, you can control
the behavior with `--on-collision`:

- `overwrite` (default): Write the file from each bundle, recording collisions in the summary.
- `skip`: Keep the first occurrence, skip subsequent writes.
- `error`: Abort when a collision is detected.

Collisions are reported to stderr when they occur.

```sh
dtex extract -i <bundle_dir> -o output --on-collision skip
```

#### Error handling

By default, errors during directory extraction are recorded and processing continues. Use
`--strict` to abort on the first error.

```sh
dtex extract -i <bundle_dir> -o output --strict
```

#### Manifest

To track the provenance of all extracted files, use `--manifest` to write a TSV file:

```sh
dtex extract -i <bundle_dir> -o output --manifest manifest.tsv
```

The manifest contains: `output_path<TAB>source_bundle<TAB>name_hash<TAB>ext` for every extracted file.

#### Progress and output

- `--quiet`: Suppress progress and summary output (errors still print).
- `--json`: Output machine-readable JSON to stdout (human-readable summary still goes to stderr unless `--quiet`).
- `--verbose`: Enable verbose output (including Oodle debug logging).

### Find files in bundles

```sh
# Find all files with a given extension
dtex find lua -i <bundle_dir>

# Without an extension, print a per-extension count summary
dtex find -i <bundle_dir>

# Output as JSON
dtex find lua -i <bundle_dir> --json
```

### Validate bundle files

```sh
# Check classification of files in a directory
dtex validate -i <bundle_dir>

# Output as JSON
dtex validate -i <bundle_dir> --json
```

### Other commands

```sh
# Dump all extension/name hashes from a bundle index
dtex dump-hashes -i <bundle_file>

# Scan bundle content for path strings to build a dictionary
dtex scan -i <bundle_file> [-i <bundle_file> ...] -o dictionary.txt
dtex scan -i <bundle_file> -o dictionary.txt --merge existing_dictionary.txt

# Check dictionary coverage against bundle index hashes
dtex coverage -d dictionary.txt -i <bundle_file> [-i <bundle_file> ...]
```

Lua files are written as standard LuaJIT bytecode (the Fatshark wrapper is stripped and the
magic/version bytes restored automatically).

### Global options

| Option | Description | Default |
|---|---|---|
| `--oodle-lib <path>` | Path to the Oodle shared library | Platform default (see Installation) |
| `--game-dir <path>` | Darktide game directory (for Steam auto-discovery and Windows Oodle DLL) | Auto-discovered or none |
| `--verbose` | Enable verbose output (including Oodle debug output) | false |
| `--quiet` | Suppress stderr progress/summary (errors still print) | false |
| `--json` | Output machine-readable JSON for data commands | false |

### Extract options

| Option | Description |
|---|---|
| `--raw` | Write files as `<name_hash>.<ext_hash>` without name resolution |
| `--dictionary <path>` | Resolve `name_hash` via a dictionary file |
| `--lua-chunknames` | Name Lua files using the chunkname from bytecode debug info |
| `--on-collision <mode>` | How to handle output path collisions: `overwrite`, `skip`, or `error` (default: `overwrite`) |
| `--strict` | Abort on first error (instead of continuing) |
| `--manifest <path>` | Write manifest TSV of extracted files |

## Lua extraction pipeline

Lua source can be fully recovered in two stages: `dtex` extracts Darktide's
custom-wrapped LuaJIT bytecode into standard LuaJIT bytecode, then a LuaJIT
decompiler turns it back into readable Lua.

```sh
# 1. Extract Lua into standard LuaJIT bytecode
dtex extract -i <bundle_dir> -o <bytecode_dir> --lua-chunknames lua

# 2. Decompile the bytecode into Lua source
luadejit <bytecode_dir> -o <source_dir>
```

`luadejit` is a LuaJIT 2.x decompiler (`luadejit --help` lists options such as
`--strip-dir-prefix` to trim a common leading path from chunkname-derived
filenames). See [`docs/luajit-bytecode-format.md`](docs/luajit-bytecode-format.md)
for the wrapper format and normalization details.

## Dictionary workflow

For non-Lua types, build a MurmurHash64A reverse-lookup dictionary by scanning decompressed
bundle content for path strings, then use it to name extracted files.

```sh
# 1. Build a dictionary
dtex scan -i bundle1.bundle -i bundle2.bundle -o dictionary.txt

# 2. Check coverage
dtex coverage -d dictionary.txt -i bundle1.bundle -i bundle2.bundle

# 3. Extract with resolved names (unresolved files fall back to <ext>/<name_hash>)
dtex extract -i bundle1.bundle -o output --dictionary dictionary.txt
```

Files resolved by the dictionary are written to their original paths; unresolved files land
under their extension directory by hex hash. To grow the dictionary incrementally:

```sh
dtex scan -i new_bundle.bundle -o dictionary.txt --merge dictionary.txt
```

## Library

Add `darktide-bundle` to your `Cargo.toml`:

```toml
[dependencies]
darktide-bundle = { path = "crates/darktide-bundle" }
```

```rust
use darktide_bundle::{Bundle, Oodle};

let oodle = Oodle::load("liboo2corelinux64.so.9")?; // Linux
// let oodle = Oodle::load("oo2core_9_win64.dll")?; // Windows
let mut bundle = Bundle::open("data.bundle")?;

let index = bundle.read_index()?;
let files = bundle.extract_files(&oodle)?;

for file in &files {
    println!("hash: 0x{:016x}", file.name);
}
```

Key types:

- `Bundle` — opens and parses a `.bundle` file; provides `read_index()` and `extract_files()`.
- `Oodle` — loads the Oodle shared library at runtime via `libloading`.
- `FileEntry` — a decompressed file with name hash, extension hash, variant data, and content.
- `IndexEntry` — one entry from the bundle index (extension hash, name hash, mode).
- `Dictionary` — MurmurHash64A reverse-lookup dictionary for name resolution.
- `normalize_luajit` / `denormalize_luajit` / `is_darktide_wrapped` — Lua bytecode normalization (lossless, reversible).
- `scan_strings` — scan binary data for path-like strings.
- `murmur_hash64` / `lookup_extension` — hashing and extension lookup.

## FFI library

The `darktide-ffi` crate compiles to a shared library (`cdylib`) and static library
(`staticlib`) with a C-compatible API. It wraps `darktide-bundle` using opaque handles for
use from other languages.

Pre-built FFI artifacts ship with each [Release](https://github.com/ModifAmorphic/darktide-extractor/releases): the shared library (`libdarktide_ffi.so` / `darktide_ffi.dll`), static library (`libdarktide_ffi.a` / `darktide_ffi.lib`), and `darktide.h` are bundled in the platform archive alongside the `dtex` binary.

A ready-to-use C header is included at [`crates/darktide-ffi/darktide.h`](crates/darktide-ffi/darktide.h); C and C++ consumers can `#include` it directly. The function list below is authoritative if you need to hand-transcribe signatures.

Exported functions:

- `darktide_oodle_load(path)` / `darktide_oodle_free(oodle)`
- `darktide_bundle_open(path)` / `darktide_bundle_free(bundle)`
- `darktide_bundle_read_index(bundle)` / `darktide_bundle_index_free(index)`
- `darktide_bundle_index_count(index)` / `darktide_bundle_index_entry(bundle, idx, out)`
- `darktide_bundle_extract_all(bundle, oodle)` / `darktide_bundle_files_free(files)`
- `darktide_bundle_files_count(files)` / `darktide_bundle_file_entry(files, idx, out)`
- `darktide_bundle_file_data(files, idx, out_buf, out_len)`
- `darktide_murmur_hash64(data, len)` / `darktide_lookup_extension(hash)`

All functions taking raw pointers are `unsafe extern "C"`; see the `# Safety` docs on each.
`darktide_bundle_file_data` copies `min(data_len, out_len)` bytes — the caller must size the
buffer using `data_len` from `darktide_bundle_file_entry`.

## Building

```sh
cargo build --release
```

Artifacts:

- CLI: `target/release/dtex` (`.exe` on Windows)
- FFI shared: `target/release/libdarktide_ffi.so` (Linux) / `darktide_ffi.dll` (Windows)
- FFI static: `target/release/libdarktide_ffi.a`

## Project structure

- Root crate (`darktide-extractor-cli`) — CLI binary `dtex` (`list`, `extract`, `dump-hashes`, `scan`, `coverage`, `find`, `validate`); also the Cargo workspace root.
- `crates/darktide-bundle` — core library (bundle parsing, Oodle decompression, MurmurHash64A, Lua normalization, dictionary).
- `crates/darktide-ffi` — C ABI wrapper (`cdylib` + `staticlib`).
- `docs/` — technical specifications.

The root CLI crate and `darktide-ffi` both depend on `darktide-bundle`, which has
no internal crate dependencies. All workspace crates share a single version that
is bumped together on each release.

## Documentation

- [`docs/bundle-format.md`](docs/bundle-format.md) — the `.bundle` binary layout.
- [`docs/luajit-bytecode-format.md`](docs/luajit-bytecode-format.md) — Darktide's custom LuaJIT wrapper and normalization.
- [`docs/oodle-library.md`](docs/oodle-library.md) — obtaining the Oodle library from Epic's CDN.
