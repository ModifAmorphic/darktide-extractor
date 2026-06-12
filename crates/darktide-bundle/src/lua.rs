/// Extract the source name (chunkname) from a Darktide Lua file.
///
/// Darktide wraps standard LuaJIT bytecode in a custom 24-byte header.
/// The LuaJIT bytecode starts at offset 0x18 with:
///   - 4 bytes: magic (1b 46 53 82 for Darktide, or 1b 4c 4a 02 for standard)
///   - 1 byte: version at offset 0x1C
///   - 1 byte: source name length at offset 0x1D
///   - N bytes: source name string at offset 0x1E
///
/// Source names typically start with '@' followed by the file path.
/// Returns the path without the '@' prefix, or None if data is too short
/// or doesn't look like valid Lua bytecode.
pub fn extract_chunkname(data: &[u8]) -> Option<String> {
    // Need at least 0x1E bytes to reach the source name length field
    // Plus at least 1 byte for the source name itself
    if data.len() < 0x1F {
        return None;
    }

    // Check for LuaJIT magic at offset 0x18
    // Accept both Darktide custom (1b 46 53) and standard LuaJIT (1b 4c 4a)
    let magic = &data[0x18..0x1B];
    if magic != [0x1b, 0x46, 0x53] && magic != [0x1b, 0x4c, 0x4a] {
        return None;
    }

    let name_len = data[0x1D] as usize;
    if name_len == 0 {
        return None;
    }

    let name_start = 0x1E;
    let name_end = name_start + name_len;
    if name_end > data.len() {
        return None;
    }

    let name_bytes = &data[name_start..name_end];
    // Source name should be valid UTF-8
    let name = std::str::from_utf8(name_bytes).ok()?;

    if name.starts_with('@') {
        Some(name[1..].to_string())
    } else {
        Some(name.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
