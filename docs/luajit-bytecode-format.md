# Darktide LuaJIT Bytecode Format

Darktide compiles Lua source to LuaJIT 2.x bytecode, then wraps each bytecode file in a
custom Fatshark header with two non-standard byte changes. `dtex extract` normalizes these
files back to standard LuaJIT bytecode on write (see
[`normalize_luajit` in `crates/darktide-bundle/src/lua.rs`](../crates/darktide-bundle/src/lua.rs)).

## Wrapper layout

A 24-byte prefix precedes the LuaJIT bytecode. The on-disk file is:

| Offset | Size | Field | Value |
|--------|------|-------|-------|
| `0x00` | 4 | Reserved | `0x00000000` |
| `0x04` | 4 | Bytecode size (LE u32) | `filesize - 24` |
| `0x08` | 4 | Constant | `0x28000000` (40) |
| `0x0C` | 4 | Constant | `0x02000000` (2) |
| `0x10` | 4 | Reserved | `0x00000000` |
| `0x14` | 4 | Bytecode size + 40 (LE u32) | `(filesize - 24) + 40` |
| `0x18` | 3 | LuaJIT magic | `1b 46 53` ("FS", Fatshark) — standard LuaJIT is `1b 4c 4a` ("LJ") |
| `0x1B` | 1 | Version | `0x82` (Darktide) — standard LuaJIT 2.x is `0x02` |
| `0x1C` | 1 | Flags | `0x00` (no strip / little-endian / no FFI / no FR2) |
| `0x1D` | variable | Chunkname length | ULEB128-encoded |
| after length | variable | Chunkname string | Starts with `@`, ends with `.lua` or `.luad` |

After the chunkname, the standard LuaJIT prototype stream begins.

## Non-standard modifications

Two bytes differ from stock LuaJIT 2.x, verified across all 9,648 bytecode files in the
Darktide corpus:

- **Magic bytes:** `1b 46 53` ("FS", Fatshark signature) instead of standard `1b 4c 4a`
  ("LJ"). Present in 9,648 / 9,648 files.
- **Version byte:** `0x82` instead of standard `0x02`. Present in 9,648 / 9,648 files. Note
  `0x82 = 0x02 | 0x80` — the low 7 bits are the real LuaJIT version; the high bit is a
  Fatshark marker.

Everything else in the LuaJIT portion (flags, chunkname encoding, prototype/instruction
stream, constants) is byte-identical to standard LuaJIT 2.x.

## The 24-byte prefix is deterministic

Every field in the prefix is either a constant or a function of the file length (verified
uniform across all 9,648 bytecode files). No unique data is stored in the prefix. This makes
the normalize/denormalize transform lossless and fully reversible.

## Chunkname (source path) recovery

The chunkname at offset `0x1D` holds the original source path, e.g.:

```
@scripts/settings/terror_event/terror_event_templates/terror_events_km_enforcer.lua
```

The length is ULEB128-encoded (variable-length; names >= 128 bytes use a multi-byte length).
The name string starts immediately after the decoded length bytes, so its offset is not
fixed. `dtex extract --lua-chunknames` reads this path and uses it for the output filename.
This is the primary mechanism for recovering human-readable Lua filenames — no dictionary is
needed.

## Normalization

`normalize_luajit()` converts a Darktide-wrapped file to standard LuaJIT bytecode by:

1. Stripping the 24-byte prefix.
2. Restoring the magic (`1b 46 53` -> `1b 4c 4a`).
3. Restoring the version (`0x82` -> `0x02`).

The inverse `denormalize_luajit()` rebuilds the original wrapper from constants and length,
so the transform round-trips exactly. See
[`tests/normalize.rs`](../crates/darktide-bundle/tests/normalize.rs) for the round-trip test.

## Edge case: plaintext Lua source

A small number of bundle entries contain plaintext Lua source (not compiled bytecode). These
have a different prefix (`0x08` = 28 instead of 40) and lack the `FS` magic, so
`normalize_luajit()` passes them through unchanged. Standard LuaJIT tooling skips them (they
do not begin with the bytecode magic).
