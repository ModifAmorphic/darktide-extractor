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

See [`docs/bundle-format.md`](docs/bundle-format.md) for the binary format details and
[`docs/luajit-bytecode-format.md`](docs/luajit-bytecode-format.md) for Lua-specific handling.

## Installation

### Prerequisites: the Oodle library

`dtex` dynamically loads the Oodle shared library at runtime. Place the platform-appropriate
file next to the `dtex` binary (or pass its path with `--oodle-lib`):

- Linux: `liboo2corelinux64.so.9` (Oodle 2.9.14; vendored in this repo)
- Windows: `oo2core_9_win64.dll` (Oodle 2.9.10; obtain separately)

See [`docs/oodle-library.md`](docs/oodle-library.md) for how to obtain the Windows DLL from
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
# All files
dtex extract -i <bundle_file> -o <output_dir>

# Filter by extension
dtex extract -i <bundle_file> -o <output_dir> lua

# Raw mode: <name_hash>.<ext_hash>, no resolution
dtex extract -i <bundle_file> -o <output_dir> --raw

# Dictionary-based name resolution
dtex extract -i <bundle_file> -o <output_dir> --dictionary dictionary.txt

# Lua files named by their chunkname (original source paths)
dtex extract -i <bundle_file> -o <output_dir> --lua-chunknames lua
```

Lua files are written as standard LuaJIT bytecode (the Fatshark wrapper is stripped and the
magic/version bytes restored automatically).

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

### Global options

| Option | Description | Default |
|---|---|---|
| `--oodle-lib <path>` | Path to the Oodle shared library | Platform default (see Installation) |

### Extract options

| Option | Description |
|---|---|
| `--raw` | Write files as `<name_hash>.<ext_hash>` without name resolution |
| `--dictionary <path>` | Resolve `name_hash` via a dictionary file |
| `--lua-chunknames` | Name Lua files using the chunkname from bytecode debug info |

## Lua extraction pipeline

Lua is the primary asset type whose original path is fully recoverable. The path is embedded
in the bytecode debug info (chunkname), so `--lua-chunknames` reconstructs the in-game
directory structure (`scripts/`, `dialogues/`, `content/`, `core/`).

```sh
# 1. Extract Lua to standard LuaJIT bytecode, named by source path
dtex extract -i <bundle_dir> -o <output_dir> --lua-chunknames lua

# 2. Decompile with a LuaJIT 2.x decompiler, e.g. luajit-decompiler-v2
luajit-decompiler-v2 <output_dir> -o <final_output> --organized -s -f
```

At scale across all Darktide bundles: 14,678 bundles scanned, 422 contain Lua, 9,649 Lua
files extracted (9,648 bytecode), 9,646 decompile successfully (99.97%). Output breaks down
as `scripts/` (4,801), `dialogues/` (3,437), `content/` (1,402), `core/` (6).

See [`docs/luajit-bytecode-format.md`](docs/luajit-bytecode-format.md) for the wrapper format
and normalization details.

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

- `crates/darktide-bundle` — core library (bundle parsing, Oodle decompression, MurmurHash64A, Lua normalization, dictionary).
- `crates/darktide-extractor-cli` — CLI binary `dtex` (`list`, `extract`, `dump-hashes`, `scan`, `coverage`).
- `crates/darktide-ffi` — C ABI wrapper (`cdylib` + `staticlib`).
- `docs/` — technical specifications.

Dependency chain: `darktide-extractor-cli` and `darktide-ffi` both depend on
`darktide-bundle`, which has no internal crate dependencies.

## Documentation

- [`docs/bundle-format.md`](docs/bundle-format.md) — the `.bundle` binary layout.
- [`docs/luajit-bytecode-format.md`](docs/luajit-bytecode-format.md) — Darktide's custom LuaJIT wrapper and normalization.
- [`docs/oodle-library.md`](docs/oodle-library.md) — obtaining the Oodle library from Epic's CDN.
