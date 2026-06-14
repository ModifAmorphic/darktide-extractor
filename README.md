# darktide-extractor

Tools for working with Darktide resource bundles. This project provides a Rust library for parsing and extracting `.bundle` files, a CLI for interactive use, and a C-compatible FFI library for integration into other projects. Supports Linux and Windows.

Darktide bundles use Oodle compression and store file names as MurmurHash64A hashes rather than plain text. This project handles decompression, hash resolution, and file extraction.

## Usage

### CLI

**List files in a bundle:**
```sh
dtex list -i <bundle_file> [extension]
```

**Extract files from a bundle:**
```sh
dtex extract -i <bundle_file> -o <output_dir> [extension]
```

**Extract files in raw mode (no hash resolution):**
```sh
dtex extract -i <bundle_file> -o <output_dir> --raw
```

**Extract files with dictionary-based name resolution:**
```sh
dtex extract -i <bundle_file> -o <output_dir> --dictionary dictionary.txt
```

**Extract Lua files with chunkname-based naming:**
```sh
dtex extract -i <bundle_file> -o <output_dir> --lua-chunknames lua
```

**Dump all extension and name hashes from a bundle:**
```sh
dtex dump-hashes -i <bundle_file>
```

**Scan bundle content for path strings and build a dictionary:**
```sh
dtex scan -i <bundle_file> [-i <bundle_file> ...] -o dictionary.txt
```

**Scan with merge (append to existing dictionary):**
```sh
dtex scan -i <bundle_file> -o dictionary.txt --merge existing_dictionary.txt
```

**Check dictionary coverage against bundle index hashes:**
```sh
dtex coverage -d dictionary.txt -i <bundle_file> [-i <bundle_file> ...]
```

#### Global Options

| Option | Description | Default |
|---|---|---|
| `--oodle-lib <path>` | Path to the Oodle shared library | Platform default (see below) |

#### Extract Options

| Option | Description |
|---|---|
| `--raw` | Extract raw data without file name resolution |
| `--dictionary <path>` | Path to dictionary file for name resolution |
| `--lua-chunknames` | Name Lua files using their chunkname from bytecode debug info |
| | Lua files are automatically normalized to standard LuaJIT bytecode (no flag required) |

#### Examples

List all Lua files in a bundle:
```sh
dtex list -i data.bundle lua
```

Extract all files to a directory:
```sh
dtex extract -i data.bundle -o output
```

Extract with a custom Oodle library path:
```sh
dtex --oodle-lib path/to/liboo2core.so extract -i data.bundle -o output
```

### Dictionary Workflow

Darktide bundles store file names as MurmurHash64A hashes. The dictionary feature scans decompressed bundle content for plaintext path strings, builds a hash-to-path mapping, and uses it to name extracted files with their original paths.

**Note:** Dictionary-based name resolution has 0% coverage for `name_hash` fields. The name hashes are content-addressed rather than derived from file paths, so scanning for path strings cannot recover them. The dictionary workflow remains useful for other file types and hash lookups. For Lua files specifically, use the `--lua-chunknames` flag instead, which recovers paths directly from bytecode debug info.

**Step 1: Scan bundles to build a dictionary:**
```sh
dtex scan -i bundle1.bundle -i bundle2.bundle -o dictionary.txt
```

**Step 2: Check coverage:**
```sh
dtex coverage -d dictionary.txt -i bundle1.bundle -i bundle2.bundle
```

**Step 3: Extract with resolved names:**
```sh
dtex extract -i bundle1.bundle -o output --dictionary dictionary.txt
```

Files that are resolved by the dictionary are written to their original paths (e.g. `data/92/92a98617c33549427`). Unresolved files fall back to hash-based naming under their extension directory.

To incrementally grow the dictionary as more bundles are scanned:
```sh
dtex scan -i new_bundle.bundle -o dictionary.txt --merge dictionary.txt
```

## Bundle Extraction to Friendly Paths

This section documents the full pipeline for extracting Darktide bundle contents to human-readable file paths. The Lua extraction path is the primary recommended workflow for recovering script source.

### Lua Extraction Pipeline

Lua files are a special case: they do not require a dictionary. The original file paths are embedded in the bytecode debug info (the chunkname field), so `--lua-chunknames` recovers them directly.

**Step 1: Extract Lua files with chunkname-based naming (dtex now outputs standard LuaJIT bytecode directly — no patching needed):**
```sh
dtex extract -i <bundle_dir> -o <output_dir> --lua-chunknames lua
```

The `--lua-chunknames` flag reads the source name from each Lua bytecode file's debug info and uses it for the output path. Files are organized into subdirectories matching their in-game paths (e.g. `scripts/`, `dialogues/`, `content/`).

**Step 2: Decompile with luajit-decompiler-v2:**
```sh
luajit-decompiler-v2 <output_dir> -o <final_output> --organized -s -f
```

The `--organized` flag preserves the directory structure from the chunknames.

### Results

Using this pipeline across all Darktide bundles:

| Metric | Count |
|---|---|
| Bundles scanned | 14,678 |
| Bundles containing Lua files | 422 |
| Total Lua files extracted | 9,649 |
| Bytecode files | 9,648 |
| Successfully decompiled | 9,646 (99.97%) |

Output directory breakdown:

| Directory | Files |
|---|---|
| `scripts/` | 4,801 |
| `dialogues/` | 3,437 |
| `content/` | 1,402 |
| `core/` | 6 |

### Lua Bytecode Notes

**Note:** `dtex extract` now automatically normalizes Lua files to standard LuaJIT bytecode, so manual patching is no longer required. The format documentation below is preserved for reference.

Darktide wraps standard LuaJIT bytecode in a custom header. This is present in every `.lua` file extracted from bundles and must be handled before standard LuaJIT tools can process the files.

#### Custom Header

A 24-byte prefix precedes the LuaJIT bytecode:

| Offset | Size | Field | Value |
|--------|------|-------|-------|
| `0x00` | 4 | Reserved | `0x00000000` |
| `0x04` | 4 | Bytecode size (LE u32) | `filesize - 24` |
| `0x08` | 4 | Constant | `0x28000000` (40) |
| `0x0C` | 4 | Constant | `0x02000000` (2) |
| `0x10` | 4 | Reserved | `0x00000000` |
 | `0x14` | 4 | Bytecode size + 40 | `filesize - 24 + 40` |
| `0x18` | 3 | LuaJIT magic | `0x1b4653` (FS) or `0x1b4c4a` (LJ) |
| `0x1B` | 1 | Version | `0x82` (Darktide) or `0x02` (standard) |
| `0x1C` | 1 | Flags | Various flags |
| `0x1D` | variable | Chunkname length (ULEB128) | ULEB128-encoded length |
| variable | variable | Chunkname string | Null-free string, prefixed with `@`. Ends with `.lua` or `.luad` |

After the source name, the standard LuaJIT bytecode begins.

#### Fatshark Custom LuaJIT Modifications

The LuaJIT bytecode within the custom header has two non-standard modifications:

- **Custom magic bytes:** Standard LuaJIT uses `1b 4c 4a` ("LJ"). Darktide uses `1b 46 53` ("FS") -- a custom Fatshark signature. All 9,648 bytecode files use this custom magic.
- **Custom version byte:** Standard LuaJIT version 2 uses byte `0x02` at offset `0x1B`. Darktide uses `0x82`. Found in 8,390 of 9,648 files.

Both must be patched back to standard values for LuaJIT decompilers to accept the files.

#### Source Name Recovery

The source name field contains the original file path, for example:
```
@scripts/settings/terror_event/terror_event_templates/terror_events_km_enforcer.lua
```

The chunkname length is ULEB128-encoded starting at offset `0x1D`. The actual name starts after the decoded length bytes.

This is the key to recovering human-readable filenames without a dictionary. The `--lua-chunknames` flag extracts and uses these paths for output file naming.

#### Clean Bytecode Extraction

To extract standard LuaJIT bytecode from a Darktide-wrapped file, skip the first `24 + chunkname_uleb128_bytes + chunkname_length` bytes. The `normalize_luajit()` function handles this automatically and returns standard LuaJIT bytecode ready for decompilation.

### Library

Add `darktide-bundle` to your `Cargo.toml`:

```toml
[dependencies]
darktide-bundle = { path = "crates/darktide-bundle" }
```

Then use it in Rust:

```rust
use darktide_bundle::{Bundle, Oodle};

let oodle = Oodle::load("liboo2corelinux64.so.9")?; // Linux
// let oodle = Oodle::load("oo2core_9_win64.dll")?; // Windows
let mut bundle = Bundle::open("data.bundle")?;

let index = bundle.read_index()?;
let files = bundle.extract_all(&oodle)?;

for file in &files {
    println!("hash: 0x{:016x}", file.name);
}
```

Key types:

- `Bundle` - Opens and parses a `.bundle` file. Provides `read_index()` and `extract_files()`.
- `Oodle` - Loads the Oodle shared library at runtime via `libloading`. Used for decompression.
- `FileEntry` - A decompressed file with name hash, extension hash, variant data, and raw content.
- `IndexEntry` - A single entry from the bundle file index (extension hash, name hash, mode).
- `Dictionary` - MurmurHash64A reverse-lookup dictionary. Maps name hashes to file paths for extraction.
- `scan_strings()` - Scan binary data for path-like strings (null or 0xFF terminated).
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

## Installation

Pre-built binaries are available on the [Releases](https://github.com/ModifAmorphic/darktide-extractor/releases) page for Linux and Windows.

After downloading, place the Oodle library in the same directory as the binary:

- Linux: `liboo2corelinux64.so.9`
- Windows: `oo2core_9_win64.dll`

The library can be obtained from the Unreal Engine CDN. See the [Oodle Library](#oodle-library) section for details.

## Building

```bash
cargo build --release
```

The CLI binary will be at `target/release/dtex`. The FFI libraries will be at `target/release/libdarktide_ffi.so` (Linux shared) or `target/release/darktide_ffi.dll` (Windows shared), and `target/release/libdarktide_ffi.a` (static).

## Oodle Library

Darktide bundles use Oodle compression (Kraken). This project requires the Oodle shared library at runtime. It is not open source, but it is distributed as a build dependency of Unreal Engine on Epic's CDN.

### Supported platforms

| Platform | Library | Version | Size |
|---|---|---|---|
| Linux | `liboo2corelinux64.so.9` | Oodle 2.9.14 | 688,096 bytes |
| Windows | `oo2core_9_win64.dll` | Oodle 2.9.10 | 637,952 bytes |

The `libloading` crate handles platform-specific dynamic loading (`dlopen` on Unix, `LoadLibrary` on Windows). The Oodle FFI signature is identical across platforms.

### Obtaining the library

1. Join the Epic Games GitHub organization: https://github.com/EpicGames/Signup (free, instant approval)
2. Download `Engine/Build/Commit.gitdeps.xml` from the [UnrealEngine repo](https://github.com/EpicGames/UnrealEngine)
3. Parse the XML to download the file. The XML has four linked elements you must trace:

```
<DependencyManifest BaseUrl="...">      -> CDN base URL
  <File Name="..." Hash="..." />         -> find the file you want by Name
  <Blob Hash="..." PackHash="..." Size="..." PackOffset="..." />  -> match Hash to File
  <Pack Hash="..." RemotePath="..." ... />                         -> match Hash to Blob.PackHash
```

4. Download using one of the methods below (Linux/macOS or Windows).

### Linux

The library is vendored in this repo. Here is exactly how it was obtained:

**File entry** (v2.9.14 Linux shared library):
```xml
<File Name="Engine/Source/Runtime/OodleDataCompression/Sdks/2.9.14/lib/Linux/liboo2corelinux64.so.9"
      Hash="ff1f6d0faa4fceaeec9d4c1a0a391160dfe78b54" />
```

**Blob** (match Hash to file):
```xml
<Blob Hash="ff1f6d0faa4fceaeec9d4c1a0a391160dfe78b54"
      Size="688096"
      PackHash="4f6c5fd233cb85f91497bd8c722fd7a89f1c657a"
      PackOffset="1399275" />
```

**Pack** (match Hash to Blob.PackHash):
```xml
<Pack Hash="4f6c5fd233cb85f91497bd8c722fd7a89f1c657a"
      RemotePath="UnrealEngine-42566482"
      Size="2087371"
      CompressedSize="1335781" />
```

**Base URL**: `https://cdn.unrealengine.com/dependencies`

**Download command** (Linux/macOS):
```sh
curl -sL "https://cdn.unrealengine.com/dependencies/UnrealEngine-42566482/4f6c5fd233cb85f91497bd8c722fd7a89f1c657a" \
  | gunzip \
  | dd bs=1 skip=1399275 count=688096 of=liboo2corelinux64.so.9
```

Result: 672KB ELF shared object, MD5 `18aa46f51f41f8c81cde1636ad486c81`.

### Windows

**File details** (v2.9.10 Windows DLL):
- Pack RemotePath: `UnrealEngine-27563807`
- Pack Hash: `51bf6515dd35ac8361c9a324b6deb1736a61240c`
- PackOffset: `1240856`
- Size: `637952`

**Download command** (Linux/macOS):
```sh
curl -sL "https://cdn.unrealengine.com/dependencies/UnrealEngine-27563807/51bf6515dd35ac8361c9a324b6deb1736a61240c" \
  | gunzip \
  | dd bs=1 skip=1240856 count=637952 of=oo2core_9_win64.dll
```

**Download command** (Windows PowerShell):
```powershell
$url = "https://cdn.unrealengine.com/dependencies/UnrealEngine-27563807/51bf6515dd35ac8361c9a324b6deb1736a61240c"
$response = Invoke-WebRequest -Uri $url -UseBasicParsing
$gzip = [System.IO.Compression.GzipStream]::new(
    [System.IO.MemoryStream]::new($response.Content),
    [System.IO.Compression.CompressionMode]::Decompress
)
$bytes = [System.IO.MemoryStream]::new()
$gzip.CopyTo($bytes)
$gzip.Dispose()
$data = $bytes.ToArray()
$offset = 1240856
$count = 637952
$dll = $data[$offset..($offset + $count - 1)]
[System.IO.File]::WriteAllBytes("oo2core_9_win64.dll", $dll)
```

Result: 623KB PE DLL, Oodle 2.9.10.

## Project Structure

- `crates/darktide-bundle` - Core library. Parses the Darktide bundle binary format, handles Oodle decompression via dynamic library loading, implements MurmurHash64A for file name resolution, and defines the on-disk types (`IndexEntry`, `FileEntry`, `FileVariant`).
- `crates/darktide-extractor-cli` - CLI binary (`dtex`). Depends on `darktide-bundle`. Provides `list`, `extract`, `dump-hashes`, `scan`, and `coverage` subcommands with argument parsing via `clap`.
- `crates/darktide-ffi` - C-compatible FFI library. Depends on `darktide-bundle`. Exposes the core functionality through opaque handles and `#[repr(C)]` structs for integration from other languages. Compiled as both `cdylib` and `staticlib`.

Dependency chain: `darktide-extractor-cli` and `darktide-ffi` both depend on `darktide-bundle`. The bundle crate has no internal crate dependencies.
