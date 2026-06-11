# darktide-extractor

Tools for working with Darktide resource bundles. This project provides a Rust library for parsing and extracting `.bundle` files, a CLI for interactive use, and a C-compatible FFI library for integration into other projects.

Darktide bundles use Oodle compression and store file names as MurmurHash64A hashes rather than plain text. This project handles decompression, hash resolution, and file extraction.

## Usage

### CLI

**List files in a bundle:**
```bash
darktide-cli list -i <bundle_file> [extension]
```

**Extract files from a bundle:**
```bash
darktide-cli extract -i <bundle_file> -o <output_dir> [extension]
```

**Extract files in raw mode (no hash resolution):**
```bash
darktide-cli extract -i <bundle_file> -o <output_dir> --raw
```

**Dump all extension and name hashes from a bundle:**
```bash
darktide-cli dump-hashes -i <bundle_file>
```

#### Global Options

| Option | Description | Default |
|---|---|---|
| `--oodle-lib <path>` | Path to the Oodle shared library | `liboo2corelinux64.so.9` |

#### Examples

List all Lua files in a bundle:
```bash
darktide-cli list -i data.bundle lua
```

Extract all files to a directory:
```bash
darktide-cli extract -i data.bundle -o output/
```

Extract with a custom Oodle library path:
```bash
darktide-cli --oodle-lib /path/to/liboo2core.so extract -i data.bundle -o output/
```

### Library

Add `darktide-bundle` to your `Cargo.toml`:

```toml
[dependencies]
darktide-bundle = { path = "crates/darktide-bundle" }
```

Then use it in Rust:

```rust
use darktide_bundle::{Bundle, Oodle};

let oodle = Oodle::load("liboo2corelinux64.so.9")?;
let mut bundle = Bundle::open("data.bundle")?;

let index = bundle.read_index()?;
let files = bundle.extract_all(&oodle)?;

for file in &files {
    println!("hash: 0x{:016x}", file.name);
}
```

Key types:

- `Bundle` - Opens and parses a `.bundle` file. Provides `read_index()` and `extract_all()`.
- `Oodle` - Loads the Oodle shared library at runtime via `libloading`. Used for decompression.
- `FileEntry` - A decompressed file with name hash, extension hash, variant data, and raw content.
- `IndexEntry` - A single entry from the bundle file index (extension hash, name hash, mode).
- `murmur_hash64()` - Compute the MurmurHash64A of data, matching the bundle format.
- `lookup_extension()` - Resolve a known extension hash back to its string name.

### FFI Library

The `darktide-ffi` crate compiles to a shared library (`cdylib`) and static library (`staticlib`) with a C-compatible API. It wraps `darktide-bundle` using opaque handles for use from other languages.

Exported functions:

- `darktide_oodle_load(path)` / `darktide_oodle_free(oodle)` - Load and free the Oodle library
- `darktide_bundle_open(path)` / `darktide_bundle_free(bundle)` - Open and close a bundle
- `darktide_bundle_file_count(bundle)` - Get number of files in the index
- `darktide_bundle_read_index(bundle)` / `darktide_bundle_index_free(index)` - Read and free the file index
- `darktide_bundle_index_entry(bundle, idx, out)` - Get an index entry by position
- `darktide_bundle_extract_all(bundle, oodle)` / `darktide_bundle_files_free(files)` - Extract and free all files
- `darktide_bundle_file_entry(files, idx, out)` - Get file metadata
- `darktide_bundle_file_data(files, idx, out_buf, out_len)` - Copy file data into a buffer
- `darktide_murmur_hash64(data, len)` - Compute MurmurHash64A
- `darktide_lookup_extension(hash)` - Lookup extension name by hash

## Building

```bash
cargo build --release
```

The CLI binary will be at `target/release/darktide-cli`. The FFI libraries will be at `target/release/libdarktide_ffi.so` (shared) and `target/release/libdarktide_ffi.a` (static).

## Project Structure

- `crates/darktide-bundle` - Core library. Parses the Darktide bundle binary format, handles Oodle decompression via dynamic library loading, implements MurmurHash64A for file name resolution, and defines the on-disk types (`IndexEntry`, `FileEntry`, `FileVariant`).
- `crates/darktide-cli` - CLI binary. Depends on `darktide-bundle`. Provides `list`, `extract`, and `dump-hashes` subcommands with argument parsing via `clap`.
- `crates/darktide-ffi` - C-compatible FFI library. Depends on `darktide-bundle`. Exposes the core functionality through opaque handles and `#[repr(C)]` structs for integration from other languages. Compiled as both `cdylib` and `staticlib`.

Dependency chain: `darktide-cli` and `darktide-ffi` both depend on `darktide-bundle`. The bundle crate has no internal crate dependencies.
