# Darktide Bundle Format

Darktide game assets are packaged in `.bundle` files: Oodle (Kraken) compressed binary
archives whose file names and extensions are stored as MurmurHash64A hashes rather than
plaintext. This documents the on-disk format as parsed by
[`crates/darktide-bundle/src/bundle.rs`](../crates/darktide-bundle/src/bundle.rs).

## File layout

```
[ Header (12 B) ][ TypeData (256 B) ][ Index (20 B * num_files) ][ ChunkStream ][ Chunks... ]
```

### Header (12 bytes)

| Offset | Size | Field |
|--------|------|-------|
| `0x00` | 8 | Magic (LE u64): `0x00000003f0000008` or `0x00000003f0000007` |
| `0x08` | 4 | `num_files` (LE u32) |

### TypeData (256 bytes)

Opaque block, skipped during parsing.

### Index (`num_files` * 20 bytes)

Each entry is three MurmurHash64A-derived fields:

| Offset | Size | Field |
|--------|------|-------|
| `+0` | 8 | `ext_hash` — MurmurHash64A of the file extension |
| `+8` | 8 | `name_hash` — MurmurHash64A of the file name |
| `+16` | 4 | `mode` (flags) |

`ext_hash` is resolved to a known extension string via `lookup_extension()` (about 50 known
types: `lua`, `texture`, `package`, `bones`, etc.). `name_hash` is **content-addressed**, not
derived from the file path, so it cannot be recovered by scanning for path strings (see
Limitations in the README).

### Chunk stream header

After the index:

| Field | Notes |
|-------|-------|
| `num_chunks` (u32) | count of compressed chunks |
| chunk sizes (u32 * `num_chunks`) | header summary of each chunk size (parsed but redundant; per-chunk sizes are read inline below) |
| 16-byte alignment padding | based on actual file position |
| `total_size` (u32) | total decompressed payload size |
| `zero` (u32) | ignored |

### Chunks

Each chunk on disk begins with its own 4-byte size header, followed by 16-byte alignment
padding, then the chunk bytes. Decompression rule per chunk:

- If the on-disk `chunk_size == 0x80000` (512 KiB): the chunk is **stored uncompressed**;
  copy the bytes verbatim.
- Otherwise: Oodle-decompress the bytes into a 512 KiB (`0x80000`) output buffer using a
  scratch buffer of `0x80000 * 3` bytes.

The decompressed chunks concatenate to a buffer of at least `total_size` bytes (may have
trailing padding).

### Decompressed payload

Slice the decompressed buffer to `total_size`, then parse sequential file records:

```
[ ext_hash (u64) ][ name_hash (u64) ][ num_variants (u32) ][ flags (4 B) ]
  [ variant 0 ][ variant 1 ] ... [ variant N-1 ]
  [ body+tail content for all variants, concatenated ]
```

Each variant header is 14 bytes:

| Offset | Size | Field |
|--------|------|-------|
| `+0` | 4 | `kind` (u32) |
| `+4` | 1 | `unknown1` (0 or 1) |
| `+5` | 4 | `body_size` (u32) |
| `+9` | 1 | `unknown2` (always 1) |
| `+10` | 4 | `tail_size` (u32) |

The content for a file is the sum of `body_size + tail_size` across all its variants,
appended after the variant headers.

## Hashing

Name and extension hashes use **MurmurHash64A** with seed `0` and constant
`0xc6a4a7935bd1e995`, ported from the `limn` reference extractor. See
[`crates/darktide-bundle/src/hash.rs`](../crates/darktide-bundle/src/hash.rs).

## Robustness

The parser bounds-checks untrusted size fields against the file size and decompressed
buffer length to prevent allocation abuse and panics on malformed or truncated bundles
(`num_files`, `num_chunks`, `total_size`, and per-file `num_variants` are each validated).
See `Bundle::open` and `Bundle::parse_files`.

## References

- **limn** (Rust, Windows-only reference extractor): https://github.com/ManShanko/limn
