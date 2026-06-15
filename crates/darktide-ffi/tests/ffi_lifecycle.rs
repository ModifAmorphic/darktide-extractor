//! FFI integration tests covering the full handle lifecycle and API contracts.
//!
//! These tests only use the public FFI surface (the unsafe extern "C" functions).
//! They do NOT call into darktide_bundle directly except to compute expected hashes.

use darktide_bundle::hash::murmur_hash64;
use std::ffi::CString;
use std::fs::File;
use std::io::Write;
use std::ptr;

// Re-export all FFI functions and types from the library for easier testing
extern crate darktide_ffi as ffi;
pub use ffi::*;

// ---------------------------------------------------------------------------
// SCENARIO A: null/invalid pointer safety
// ---------------------------------------------------------------------------

/// Test that all FFI functions handle null pointers gracefully according to the
/// documented error convention.
#[test]
fn test_null_pointer_safety() {
    unsafe {
        // darktide_oodle_load with null path returns null
        let oodle = darktide_oodle_load(ptr::null());
        assert!(oodle.is_null());

        // darktide_bundle_open with null path returns null
        let bundle = darktide_bundle_open(ptr::null());
        assert!(bundle.is_null());

        // darktide_bundle_read_index with null bundle returns null
        let index = darktide_bundle_read_index(ptr::null_mut());
        assert!(index.is_null());

        // darktide_bundle_index_count with null index returns 0
        let count = darktide_bundle_index_count(ptr::null());
        assert_eq!(count, 0);

        // darktide_bundle_index_entry with null index or null out returns -1
        let mut out = DarktideIndexEntry {
            ext: 0,
            name: 0,
            mode: 0,
        };
        let result = darktide_bundle_index_entry(ptr::null(), 0, &mut out);
        assert_eq!(result, -1);

        // Create a dummy non-null index for testing out=null (we can't create a real one)
        // but we can test the out-null case
        let result = darktide_bundle_index_entry(ptr::null(), 0, ptr::null_mut());
        assert_eq!(result, -1);

        // darktide_bundle_extract_all with null bundle or null oodle returns null
        let files = darktide_bundle_extract_all(ptr::null_mut(), ptr::null_mut());
        assert!(files.is_null());

        // darktide_bundle_files_count with null files returns 0
        let count = darktide_bundle_files_count(ptr::null());
        assert_eq!(count, 0);

        // darktide_bundle_file_entry with null files or null out returns -1
        let mut out_file = DarktideFileEntry {
            ext: 0,
            name: 0,
            num_variants: 0,
            data_len: 0,
        };
        let result = darktide_bundle_file_entry(ptr::null(), 0, &mut out_file);
        assert_eq!(result, -1);

        let result = darktide_bundle_file_entry(ptr::null(), 0, ptr::null_mut());
        assert_eq!(result, -1);

        // darktide_bundle_file_data with null files or null out_buf returns -1
        let mut buf = [0u8; 16];
        let result = darktide_bundle_file_data(ptr::null(), 0, buf.as_mut_ptr(), 16);
        assert_eq!(result, -1);

        let result = darktide_bundle_file_data(ptr::null(), 0, ptr::null_mut(), 16);
        assert_eq!(result, -1);

        // Free functions with null must be no-ops (not crash)
        darktide_oodle_free(ptr::null_mut());
        darktide_bundle_free(ptr::null_mut());
        darktide_bundle_index_free(ptr::null_mut());
        darktide_bundle_files_free(ptr::null_mut());
    }
}

// ---------------------------------------------------------------------------
// SCENARIO B: Oodle load/free round-trip with the vendored library
// ---------------------------------------------------------------------------

/// Test loading and freeing the vendored Oodle library.
#[test]
fn test_oodle_load_free() {
    unsafe {
        let oodle_path = if cfg!(target_os = "windows") {
            // Windows DLL at repo root
            CString::new("../../oo2core_9_win64.dll").unwrap()
        } else {
            // Linux library at repo root
            CString::new("../../liboo2corelinux64.so.9").unwrap()
        };

        let oodle = darktide_oodle_load(oodle_path.as_ptr());
        assert!(!oodle.is_null(), "Failed to load Oodle library");

        darktide_oodle_free(oodle);
    }
}

// ---------------------------------------------------------------------------
// SCENARIO C: full bundle lifecycle on a synthetic bundle
// ---------------------------------------------------------------------------

/// Test the full FFI lifecycle with a synthetic bundle file.
/// This test builds a minimal .bundle file with one uncompressed file.
#[test]
fn test_synthetic_bundle_lifecycle() {
    const CHUNK_SIZE: u32 = 0x80000; // 524288 bytes

    // Compute hashes
    let ext_hash = murmur_hash64(b"lua");
    let name_hash = murmur_hash64(b"test_file");
    let body = b"hello, darktide!"; // 16 bytes

    unsafe {
        // First, load Oodle (needed for extract_all)
        let oodle_path = if cfg!(target_os = "windows") {
            CString::new("../../oo2core_9_win64.dll").unwrap()
        } else {
            CString::new("../../liboo2corelinux64.so.9").unwrap()
        };
        let oodle = darktide_oodle_load(oodle_path.as_ptr());
        assert!(!oodle.is_null(), "Failed to load Oodle library");

        // Build synthetic bundle
        let mut bundle_data = Vec::new();

        // Header: 12 bytes (magic u64 LE, num_files u32 LE)
        bundle_data.extend_from_slice(&0x00000003f0000008u64.to_le_bytes());
        bundle_data.extend_from_slice(&1u32.to_le_bytes());

        // TypeData: 256 bytes all zeros
        bundle_data.extend_from_slice(&[0u8; 256]);

        // Index: 1 entry of 20 bytes (ext_hash u64, name_hash u64, mode u32)
        bundle_data.extend_from_slice(&ext_hash.to_le_bytes());
        bundle_data.extend_from_slice(&name_hash.to_le_bytes());
        bundle_data.extend_from_slice(&0u32.to_le_bytes());

        // Chunk stream header:
        // num_chunks u32 = 1
        bundle_data.extend_from_slice(&1u32.to_le_bytes());

        // chunk sizes array: 1 × u32 = CHUNK_SIZE
        bundle_data.extend_from_slice(&CHUNK_SIZE.to_le_bytes());

        // Alignment padding: ((16 - (pos % 16)) % 16)
        let pos = bundle_data.len() as u64;
        let padding = ((16 - (pos % 16)) % 16) as usize;
        bundle_data.extend_from_slice(&vec![0u8; padding]);

        // FileEntry payload length (what we'll put in the chunk)
        // FileEntry layout:
        //   ext_hash u64 (8)
        //   name_hash u64 (8)
        //   num_variants u32 (4)
        //   flags [4]
        //   variant 0 header (14): kind u32 (4), unknown1 u8 (1), body_size u32 (4), unknown2 u8 (1), tail_size u32 (4)
        //   body content: body_size bytes
        //   (no tail)
        let body_size: u32 = body.len() as u32;
        let file_entry_len = 8 + 8 + 4 + 4 + 14 + body_size;

        // total_size u32 = file_entry_len
        bundle_data.extend_from_slice(&file_entry_len.to_le_bytes());

        // zero u32
        bundle_data.extend_from_slice(&0u32.to_le_bytes());

        // Chunk itself:
        // 4-byte size header = CHUNK_SIZE (signals stored-uncompressed)
        bundle_data.extend_from_slice(&CHUNK_SIZE.to_le_bytes());

        // Alignment padding after chunk size header
        let pos = bundle_data.len() as u64;
        let padding = ((16 - (pos % 16)) % 16) as usize;
        bundle_data.extend_from_slice(&vec![0u8; padding]);

        // The chunk bytes: CHUNK_SIZE bytes total
        // First file_entry_len bytes are the FileEntry payload, rest are zeros
        bundle_data.extend_from_slice(&ext_hash.to_le_bytes());
        bundle_data.extend_from_slice(&name_hash.to_le_bytes());
        bundle_data.extend_from_slice(&1u32.to_le_bytes()); // num_variants
        bundle_data.extend_from_slice(&[0u8; 4]); // flags

        // Variant 0 header
        bundle_data.extend_from_slice(&0u32.to_le_bytes()); // kind
        bundle_data.extend_from_slice(&[0u8]); // unknown1
        bundle_data.extend_from_slice(&body_size.to_le_bytes()); // body_size
        bundle_data.extend_from_slice(&[1u8]); // unknown2
        bundle_data.extend_from_slice(&0u32.to_le_bytes()); // tail_size

        // Body content
        bundle_data.extend_from_slice(body);

        // Zero padding to reach CHUNK_SIZE
        let remaining = CHUNK_SIZE as usize - file_entry_len as usize;
        bundle_data.extend_from_slice(&vec![0u8; remaining]);

        // Write to a temp file
        let temp_path =
            std::env::temp_dir().join(format!("darktide_ffi_test_{}.bundle", std::process::id()));
        {
            let mut file = File::create(&temp_path).unwrap();
            file.write_all(&bundle_data).unwrap();
        } // File handle dropped here

        // Open the bundle
        let bundle_path = CString::new(temp_path.to_str().unwrap()).unwrap();
        let bundle = darktide_bundle_open(bundle_path.as_ptr());
        assert!(!bundle.is_null(), "Failed to open bundle");

        // Read index
        let index = darktide_bundle_read_index(bundle);
        assert!(!index.is_null(), "Failed to read index");

        // Check count
        let count = darktide_bundle_index_count(index);
        assert_eq!(count, 1, "Index should have 1 entry");

        // Check entry 0
        let mut out = DarktideIndexEntry {
            ext: 0,
            name: 0,
            mode: 0,
        };
        let result = darktide_bundle_index_entry(index, 0, &mut out);
        assert_eq!(result, 0, "darktide_bundle_index_entry should succeed");
        assert_eq!(out.ext, ext_hash, "ext hash mismatch");
        assert_eq!(out.name, name_hash, "name hash mismatch");
        assert_eq!(out.mode, 0, "mode should be 0");

        // Check entry 1 (out of bounds)
        let result = darktide_bundle_index_entry(index, 1, &mut out);
        assert_eq!(
            result, -1,
            "darktide_bundle_index_entry should fail for out of bounds"
        );

        // Extract all files
        let files = darktide_bundle_extract_all(bundle, oodle);
        assert!(!files.is_null(), "Failed to extract files");

        // Check files count
        let files_count = darktide_bundle_files_count(files);
        assert_eq!(files_count, 1, "Should have 1 extracted file");

        // Check file entry 0
        let mut file_out = DarktideFileEntry {
            ext: 0,
            name: 0,
            num_variants: 0,
            data_len: 0,
        };
        let result = darktide_bundle_file_entry(files, 0, &mut file_out);
        assert_eq!(result, 0, "darktide_bundle_file_entry should succeed");
        assert_eq!(file_out.data_len, 16, "data_len should be 16");
        assert_eq!(file_out.ext, ext_hash, "ext hash mismatch");
        assert_eq!(file_out.name, name_hash, "name hash mismatch");

        // Read file data
        let mut buf = [0u8; 32];
        let result = darktide_bundle_file_data(files, 0, buf.as_mut_ptr(), 32);
        assert_eq!(result, 16, "darktide_bundle_file_data should return 16");
        assert_eq!(&buf[..16], body, "file data mismatch");

        // Test truncation: provide a smaller buffer
        let mut small_buf = [0u8; 8];
        let result = darktide_bundle_file_data(files, 0, small_buf.as_mut_ptr(), 8);
        assert_eq!(
            result, 8,
            "darktide_bundle_file_data should return 8 (truncated)"
        );
        assert_eq!(&small_buf[..8], &body[..8], "truncated data mismatch");

        // Free everything in order
        darktide_bundle_files_free(files);
        darktide_bundle_index_free(index);
        darktide_bundle_free(bundle);
        darktide_oodle_free(oodle);

        // Clean up temp file
        let _ = std::fs::remove_file(&temp_path);
    }
}

// ---------------------------------------------------------------------------
// SCENARIO D: murmur_hash64 and lookup_extension through FFI
// ---------------------------------------------------------------------------

/// Test MurmurHash64A and extension lookup through the FFI.
#[test]
fn test_murmur_hash_and_extension_lookup() {
    unsafe {
        // Compute hash of "lua"
        let lua_hash = murmur_hash64(b"lua");

        // Test darktide_murmur_hash64
        let ffi_hash = darktide_murmur_hash64(b"lua".as_ptr(), 3);
        assert_eq!(ffi_hash, lua_hash, "FFI hash should match internal hash");

        // Test with null data (should return 0)
        let null_hash = darktide_murmur_hash64(ptr::null(), 3);
        assert_eq!(null_hash, 0, "null data should return 0");

        // Test with zero length (should return 0)
        let zero_len_hash = darktide_murmur_hash64(b"lua".as_ptr(), 0);
        assert_eq!(zero_len_hash, 0, "zero length should return 0");

        // Test darktide_lookup_extension with known hash
        let ext_ptr = darktide_lookup_extension(lua_hash);
        assert!(
            !ext_ptr.is_null(),
            "lookup_extension should return non-null for 'lua'"
        );
        let ext_cstr = std::ffi::CStr::from_ptr(ext_ptr);
        assert_eq!(ext_cstr.to_bytes(), b"lua", "extension should be 'lua'");

        // Test with unknown hash
        let unknown_ptr = darktide_lookup_extension(0xdeadbeef_deadbeef);
        assert!(
            unknown_ptr.is_null(),
            "lookup_extension should return null for unknown hash"
        );
    }
}
