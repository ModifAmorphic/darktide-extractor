//! Test coverage for the Oodle decompression path in Bundle::decompress_chunks.
//!
//! Builds a synthetic .bundle with one Oodle-compressed chunk, runs it through
//! Bundle::extract_files, and verifies the extracted content matches the original.
//!
//! NOTE: The bundle parser always calls OodleLZ_Decompress with `dst_size = CHUNK_SIZE`
//! (512 KiB) and the `demand_continue = 3` setting, which requires the original
//! uncompressed size to match exactly. Real Darktide bundles respect this: every
//! chunk decompresses to exactly CHUNK_SIZE bytes, and `total_size` bounds the real
//! payload within the concatenated chunks. This test mirrors that contract by building
//! a FileEntry payload that fills exactly CHUNK_SIZE.

use darktide_bundle::hash::murmur_hash64;
use darktide_bundle::{Bundle, Oodle};
use std::fs::File;
use std::io::Write;

const CHUNK_SIZE: usize = 0x80000; // 512 KiB, the parser's stored-uncompressed threshold

#[test]
fn test_oodle_decompress_round_trip() {
    // Load vendored Oodle lib from the repo root (tests run with CWD = crate dir).
    let oodle_path = if cfg!(target_os = "windows") {
        "../../oo2core_9_win64.dll"
    } else {
        "../../liboo2corelinux64.so.9"
    };
    let oodle = Oodle::load(oodle_path).expect("Failed to load Oodle library");

    // Compute hashes for the synthetic file entry.
    let ext_hash = murmur_hash64(b"lua");
    let name_hash = murmur_hash64(b"test_file");

    // Build the FileEntry payload that the decompressed chunk should contain.
    // The FileEntry + body must exactly fill CHUNK_SIZE.
    // Layout matches Bundle::parse_files in bundle.rs:
    //   ext_hash u64, name_hash u64, num_variants u32, flags [u8;4],
    //   variant 0 header (14 bytes): kind u32, unknown1 u8, body_size u32, unknown2 u8, tail_size u32,
    //   body content (body_size bytes), no tail.
    let header_size = 8 + 8 + 4 + 4 + 14; // 38 bytes for headers
    let body_size = CHUNK_SIZE - header_size; // Fill remaining space with body data
    let body_size_u32: u32 = body_size as u32;

    let mut file_entry = Vec::with_capacity(CHUNK_SIZE);
    file_entry.extend_from_slice(&ext_hash.to_le_bytes());
    file_entry.extend_from_slice(&name_hash.to_le_bytes());
    file_entry.extend_from_slice(&1u32.to_le_bytes()); // num_variants
    file_entry.extend_from_slice(&[0u8; 4]); // flags
    file_entry.extend_from_slice(&0u32.to_le_bytes()); // variant kind
    file_entry.push(0); // variant unknown1
    file_entry.extend_from_slice(&body_size_u32.to_le_bytes()); // body_size
    file_entry.push(1); // variant unknown2
    file_entry.extend_from_slice(&0u32.to_le_bytes()); // tail_size

    // Fill the body with a pattern (alternating bytes)
    for i in 0..body_size {
        file_entry.push((i % 256) as u8);
    }

    assert_eq!(
        file_entry.len(),
        CHUNK_SIZE,
        "FileEntry must exactly fill CHUNK_SIZE"
    );

    // Save the body data for comparison (Bundle::parse_files extracts only the variant content)
    let mut expected_body = Vec::with_capacity(body_size);
    for i in 0..body_size {
        expected_body.push((i % 256) as u8);
    }

    // Compress the file_entry (not just a pattern). Use a generous buffer.
    let mut compressed = vec![0u8; CHUNK_SIZE + (CHUNK_SIZE / 2) + 1024];
    let compressed_size = oodle.compress(&file_entry, &mut compressed);
    assert!(compressed_size > 0, "Oodle compress returned 0 (failure)");
    assert!(
        compressed_size < CHUNK_SIZE,
        "compressed payload must be smaller than CHUNK_SIZE so the parser takes the decompress branch"
    );
    compressed.truncate(compressed_size);

    let total_size: u32 = file_entry.len() as u32;

    // --- Build the .bundle file ---
    let mut bundle_data = Vec::with_capacity(4096);

    // Header (12 bytes): magic u64 LE, num_files u32 LE.
    bundle_data.extend_from_slice(&0x0000_0003_f000_0008u64.to_le_bytes());
    bundle_data.extend_from_slice(&1u32.to_le_bytes());

    // TypeData (256 bytes).
    bundle_data.extend_from_slice(&[0u8; 256]);

    // Index (1 entry, 20 bytes): ext_hash, name_hash, mode.
    bundle_data.extend_from_slice(&ext_hash.to_le_bytes());
    bundle_data.extend_from_slice(&name_hash.to_le_bytes());
    bundle_data.extend_from_slice(&0u32.to_le_bytes());

    // Chunk stream header.
    bundle_data.extend_from_slice(&1u32.to_le_bytes()); // num_chunks = 1
    bundle_data.extend_from_slice(&(compressed.len() as u32).to_le_bytes()); // chunk size array

    // Alignment padding: ((16 - (pos % 16)) % 16)
    let pos = bundle_data.len() as u64;
    let padding = ((16 - (pos % 16)) % 16) as usize;
    bundle_data.extend_from_slice(&vec![0u8; padding]);

    // total_size and zero.
    bundle_data.extend_from_slice(&total_size.to_le_bytes());
    bundle_data.extend_from_slice(&0u32.to_le_bytes());

    // The chunk itself: 4-byte size header (= compressed.len(), NOT CHUNK_SIZE),
    // alignment padding, then the compressed bytes.
    bundle_data.extend_from_slice(&(compressed.len() as u32).to_le_bytes());

    let pos = bundle_data.len() as u64;
    let padding = ((16 - (pos % 16)) % 16) as usize;
    bundle_data.extend_from_slice(&vec![0u8; padding]);

    bundle_data.extend_from_slice(&compressed);

    // Write to a tempfile using std-only approach (consistent with ffi_lifecycle.rs).
    let temp_path = std::env::temp_dir().join(format!(
        "darktide_bundle_test_{}.bundle",
        std::process::id()
    ));
    {
        let mut f = File::create(&temp_path).expect("create tempfile");
        f.write_all(&bundle_data).expect("write tempfile");
    }

    // Open and extract. Use a scope so bundle drops before we delete the tempfile.
    let extracted = {
        let path_str = temp_path.to_str().expect("tempfile path is utf-8");
        let mut bundle = Bundle::open(path_str).expect("open bundle");
        bundle.extract_files(&oodle).expect("extract files")
    };

    // Clean up the tempfile.
    let _ = std::fs::remove_file(&temp_path);

    // Verify exactly one file was extracted.
    assert_eq!(extracted.len(), 1, "expected exactly 1 extracted file");

    let file = &extracted[0];
    assert_eq!(file.ext, ext_hash);
    assert_eq!(file.name, name_hash);
    // Bundle::parse_files extracts only the variant content (body + tail), not the headers.
    // Since we have one variant with body_size bytes and no tail, the extracted data should be exactly the body.
    assert_eq!(
        file.data, expected_body,
        "decompressed content must match expected body"
    );
}
