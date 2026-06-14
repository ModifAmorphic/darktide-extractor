use darktide_bundle::{is_darktide_wrapped, normalize_luajit};

#[test]
fn normalize_strips_wrapper_and_restores_standard_header() {
    // A minimal standard-LuaJIT body (magic LJ, version 0x02, flags 0x00, a tiny chunkname, then dummy proto bytes)
    let standard_body: Vec<u8> = vec![
        0x1B, 0x4C, 0x4A, 0x02, // LJ magic + version
        0x00, // flags
        // ... a few dummy bytes representing the rest of the bytecode
        0x01, 0x02, 0x03, 0x04,
    ];
    // Wrap it the Darktide way
    let bc_size = standard_body.len() as u32;
    let mut wrapped = Vec::new();
    wrapped.extend_from_slice(&[0u8; 4]);
    wrapped.extend_from_slice(&bc_size.to_le_bytes());
    wrapped.extend_from_slice(&40u32.to_le_bytes());
    wrapped.extend_from_slice(&2u32.to_le_bytes());
    wrapped.extend_from_slice(&[0u8; 4]);
    wrapped.extend_from_slice(&(bc_size + 40).to_le_bytes());
    // Now the bytecode, but with FS magic + 0x82 version:
    wrapped.extend_from_slice(&[0x1B, 0x46, 0x53, 0x82]); // FS + 0x82
    wrapped.extend_from_slice(&standard_body[4..]); // rest of body unchanged

    assert!(is_darktide_wrapped(&wrapped));
    let normalized = normalize_luajit(&wrapped);
    assert_eq!(normalized.as_ref(), standard_body.as_slice());
    assert!(!is_darktide_wrapped(&normalized));
}
