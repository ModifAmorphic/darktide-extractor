//! Module-level constants for Darktide LuaJIT wrapper format.

const FATSHARK_MAGIC: [u8; 3] = [0x1B, 0x46, 0x53]; // "FS"
const LUAJIT_MAGIC: [u8; 3] = [0x1B, 0x4C, 0x4A]; // "LJ"
const WRAPPER_LEN: usize = 24; // 0x18
const DARKTIDE_VERSION: u8 = 0x82;
const STANDARD_VERSION: u8 = 0x02;

/// True iff `data` is a Darktide-wrapped LuaJIT bytecode file.
///
/// Detects the Fatshark "FS" magic at offset 0x18 (the start of the LuaJIT
/// bytecode after the 24-byte custom prefix). This is positive identification,
/// not a heuristic — verified across all 9,648 Darktide bytecode files.
pub fn is_darktide_wrapped(data: &[u8]) -> bool {
    data.len() >= WRAPPER_LEN + 4 && data[0x18..0x1B] == FATSHARK_MAGIC
}

/// Normalize a Darktide-wrapped Lua file to standard LuaJIT bytecode.
///
/// Strips the 24-byte Fatshark prefix, restores the standard magic (`FS` -> `LJ`),
/// and restores the standard version byte (`0x82` -> `0x02`).
///
/// Lossless, idempotent, and panic-free. Returns:
/// - `Cow::Owned` with the normalized bytes if the input is Darktide-wrapped.
/// - `Cow::Borrowed` of the input unchanged otherwise (already-standard LuaJIT,
///   plaintext source, non-bytecode, empty, or too-short input all pass through).
///
/// Reversible: see [`denormalize_luajit`].
pub fn normalize_luajit(data: &[u8]) -> std::borrow::Cow<'_, [u8]> {
    if is_darktide_wrapped(data) {
        let mut out = data[WRAPPER_LEN..].to_vec();
        out[0..3].copy_from_slice(&LUAJIT_MAGIC);
        out[3] = STANDARD_VERSION;
        std::borrow::Cow::Owned(out)
    } else {
        std::borrow::Cow::Borrowed(data)
    }
}

/// Reconstruct the Darktide-wrapped form from standard LuaJIT bytecode.
///
/// Exact inverse of [`normalize_luajit`]. The 24-byte prefix is fully
/// deterministic (constants + functions of length), so this regenerates the
/// original wrapped file bit-for-bit. Useful for archival and as proof of
/// losslessness.
pub fn denormalize_luajit(normalized: &[u8]) -> Vec<u8> {
    let bc_size = normalized.len() as u32;
    let mut out = Vec::with_capacity(normalized.len() + WRAPPER_LEN);
    out.extend_from_slice(&[0u8; 4]); // 0x00 reserved
    out.extend_from_slice(&bc_size.to_le_bytes()); // 0x04 bytecode_size
    out.extend_from_slice(&40u32.to_le_bytes()); // 0x08 const 40
    out.extend_from_slice(&2u32.to_le_bytes()); // 0x0C const 2
    out.extend_from_slice(&[0u8; 4]); // 0x10 reserved
    out.extend_from_slice(&(bc_size + 40).to_le_bytes()); // 0x14 size+40
    out.extend_from_slice(normalized); // bytecode
    let m = out.len() - normalized.len(); // start of bytecode in out
    out[m..m + 3].copy_from_slice(&FATSHARK_MAGIC); // LJ -> FS
    out[m + 3] = DARKTIDE_VERSION; // 0x02 -> 0x82
    out
}

/// Extract the source name (chunkname) from a Darktide Lua file.
///
/// Darktide wraps standard LuaJIT bytecode in a custom 24-byte header.
/// The LuaJIT bytecode starts at offset 0x18 with:
///   - 4 bytes: magic (1b 46 53 82 for Darktide, or 1b 4c 4a 02 for standard)
///   - 1 byte: version at offset 0x1B
///   - 1 byte: flags at offset 0x1C
///   - N bytes: chunkname length (ULEB128) starting at offset 0x1D
///   - M bytes: source name string after the decoded length bytes
///
/// Source names typically start with '@' followed by the file path.
/// Returns the path without the '@' prefix, or None if data is too short
/// or doesn't look like valid Lua bytecode.
pub fn extract_chunkname(data: &[u8]) -> Option<String> {
    // Need at least 0x1D+1 bytes to reach the chunkname length field
    if data.len() < 0x1E {
        return None;
    }

    // Check for LuaJIT magic at offset 0x18
    // Accept both Darktide custom (1b 46 53) and standard LuaJIT (1b 4c 4a)
    let magic = &data[0x18..0x1B];
    if magic != [0x1b, 0x46, 0x53] && magic != [0x1b, 0x4c, 0x4a] {
        return None;
    }

    // Decode ULEB128 chunkname length starting at offset 0x1D
    let mut p = 0x1D;
    let mut name_len: usize = 0;
    let mut shift: u32 = 0;
    loop {
        if p >= data.len() {
            return None;
        }
        let b = data[p];
        p += 1;
        name_len |= ((b & 0x7f) as usize) << shift;
        if b < 0x80 {
            break;
        }
        shift += 7;
        if shift > 35 {
            return None; // overflow guard (ULEB128 exceeds 32 bits)
        }
    }

    if name_len == 0 {
        return None;
    }

    let name_start = p;
    let name_end = name_start + name_len;
    if name_end > data.len() {
        return None;
    }

    let name_bytes = &data[name_start..name_end];
    // Source name should be valid UTF-8
    let name = std::str::from_utf8(name_bytes).ok()?;

    if let Some(stripped) = name.strip_prefix('@') {
        Some(stripped.to_string())
    } else {
        Some(name.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_wrapped(body_after_magic_version: &[u8]) -> Vec<u8> {
        // body_after_magic_version is the bytecode AFTER the 4-byte magic+version header.
        // Total LuaJIT bytecode = 4 (magic + version) + body_after_magic_version.len()
        let bytecode_len = 4 + body_after_magic_version.len();
        let mut out = Vec::with_capacity(24 + bytecode_len);
        out.extend_from_slice(&[0u8; 4]); // 0x00 reserved
        out.extend_from_slice(&(bytecode_len as u32).to_le_bytes()); // 0x04 bytecode_size
        out.extend_from_slice(&40u32.to_le_bytes()); // 0x08 const 40
        out.extend_from_slice(&2u32.to_le_bytes()); // 0x0C const 2
        out.extend_from_slice(&[0u8; 4]); // 0x10 reserved
        out.extend_from_slice(&((bytecode_len as u32) + 40).to_le_bytes()); // 0x14 size+40
        out.extend_from_slice(&FATSHARK_MAGIC); // 0x18 magic FS
        out.push(DARKTIDE_VERSION); // 0x1B version 0x82
        out.extend_from_slice(body_after_magic_version); // 0x1C+ body
        out
    }

    #[test]
    fn normalize_wrapped_produces_standard() {
        let body = vec![0x00, 0x01, 0x02, 0x03, 0x04];
        let wrapped = make_wrapped(&body);
        let normalized = normalize_luajit(&wrapped);

        // Check output starts with standard magic + version
        assert_eq!(&normalized[..4], &[0x1B, 0x4C, 0x4A, 0x02]);
        // Check length reduced by 24
        assert_eq!(normalized.len(), wrapped.len() - 24);
        // Check body preserved (body starts at input offset 0x1C, output offset 4)
        assert_eq!(&normalized[4..], &wrapped[0x1C..]);
    }

    #[test]
    fn normalize_already_standard_passthrough() {
        let standard = vec![0x1B, 0x4C, 0x4A, 0x02, 0x00, 0x01, 0x02];
        let normalized = normalize_luajit(&standard);
        assert_eq!(normalized.as_ref(), standard.as_slice());
        assert!(matches!(normalized, std::borrow::Cow::Borrowed(_)));
    }

    #[test]
    fn normalize_non_bytecode_passthrough() {
        let random = vec![0xFF, 0xFE, 0xFD, 0xFC, 0xFB, 0xFA];
        let normalized = normalize_luajit(&random);
        assert_eq!(normalized.as_ref(), random.as_slice());
    }

    #[test]
    fn normalize_plaintext_source_passthrough() {
        // Mimics the plaintext-source edge case: no FS magic at 0x18
        let plaintext = vec![
            0, 0, 0, 0, // 0x00 reserved
            0x4a, 0, 0, 0, // 0x04 bytecode_size
            0x1c, 0, 0, 0, // 0x08 const 28
            2, 0, 0, 0, // 0x0C const 2
            0, 0, 0, 0, // 0x10 reserved
            0, 0, 0, 0, // 0x14 size+40
            0, 0, 0, 0, 0, 0, 0, 0, // 0x18-0x1F no FS magic here
            b'l', b'o', b'c', b'a',
        ];
        let normalized = normalize_luajit(&plaintext);
        assert_eq!(normalized.as_ref(), plaintext.as_slice());
    }

    #[test]
    fn normalize_empty_passthrough() {
        let empty: Vec<u8> = vec![];
        let normalized = normalize_luajit(&empty);
        assert_eq!(normalized.as_ref(), empty.as_slice());
    }

    #[test]
    fn normalize_too_short_passthrough() {
        let short = vec![0u8; 10];
        let normalized = normalize_luajit(&short);
        assert_eq!(normalized.as_ref(), short.as_slice());
    }

    #[test]
    fn normalize_is_idempotent() {
        let body = vec![0x00, 0x01, 0x02];
        let wrapped = make_wrapped(&body);
        let normalized_once = normalize_luajit(&wrapped);
        let normalized_twice = normalize_luajit(&normalized_once);
        assert_eq!(normalized_twice.as_ref(), normalized_once.as_ref());
    }

    #[test]
    fn denormalize_roundtrips_wrapped() {
        let body = vec![0x00, 0x01, 0x02, 0x03, 0x04, 0x05];
        let wrapped = make_wrapped(&body);
        let normalized = normalize_luajit(&wrapped);
        let denormalized = denormalize_luajit(&normalized);
        assert_eq!(denormalized, wrapped);
    }

    #[test]
    fn test_extract_chunkname_darktide() {
        // Minimal Darktide Lua bytecode with custom header
        let mut data = vec![0u8; 0x1E + 30];
        // Custom header (24 bytes of zeros for simplicity)
        // Darktide magic at 0x18
        data[0x18] = 0x1b;
        data[0x19] = 0x46; // 'F'
        data[0x1A] = 0x53; // 'S'
        data[0x1B] = 0x82;
        // Source name length at 0x1D
        data[0x1D] = 25;
        // Source name at 0x1E
        let name = b"@scripts/test/example.lua";
        data[0x1E..0x1E + 25].copy_from_slice(name);

        let result = extract_chunkname(&data);
        assert_eq!(result, Some("scripts/test/example.lua".to_string()));
    }

    #[test]
    fn test_extract_chunkname_standard_luajit() {
        let mut data = vec![0u8; 0x1E + 13];
        data[0x18] = 0x1b;
        data[0x19] = 0x4c; // 'L'
        data[0x1A] = 0x4a; // 'J'
        data[0x1B] = 0x02;
        data[0x1D] = 13;
        let name = b"@test/foo.lua";
        data[0x1E..0x1E + 13].copy_from_slice(name);

        let result = extract_chunkname(&data);
        assert_eq!(result, Some("test/foo.lua".to_string()));
    }

    #[test]
    fn test_extract_chunkname_no_magic() {
        let data = vec![0u8; 100];
        assert_eq!(extract_chunkname(&data), None);
    }

    #[test]
    fn test_extract_chunkname_too_short() {
        let data = vec![0u8; 10];
        assert_eq!(extract_chunkname(&data), None);
    }

    #[test]
    fn test_extract_chunkname_name_without_at_prefix() {
        let mut data = vec![0u8; 0x1E + 10];
        data[0x18] = 0x1b;
        data[0x19] = 0x46;
        data[0x1A] = 0x53;
        data[0x1B] = 0x82;
        data[0x1D] = 10;
        let name = b"some_name_";
        data[0x1E..0x1E + 10].copy_from_slice(name);

        let result = extract_chunkname(&data);
        assert_eq!(result, Some("some_name_".to_string()));
    }

    #[test]
    fn test_extract_chunkname_multibyte_uleb128_length() {
        let name_len = 200usize;
        let mut data = vec![0u8; 0x1D + 2 + name_len]; // 0x1D + 2 ULEB128 bytes + name
                                                       // Custom header (24 bytes of zeros for simplicity)
                                                       // Darktide magic at 0x18
        data[0x18] = 0x1b;
        data[0x19] = 0x46; // 'F'
        data[0x1A] = 0x53; // 'S'
        data[0x1B] = 0x82;
        // ULEB128 length at 0x1D: 200 = 0xC8, 0x01
        data[0x1D] = 0xC8; // low 7 bits = 0x48, continuation bit = 1
        data[0x1E] = 0x01; // high bits = 1
                           // Source name starts at 0x1F
        let name_vec = vec![b'a'; name_len];
        data[0x1F..0x1F + name_len].copy_from_slice(&name_vec);

        let result = extract_chunkname(&data);
        assert_eq!(result, Some("a".repeat(name_len)));
    }
}
