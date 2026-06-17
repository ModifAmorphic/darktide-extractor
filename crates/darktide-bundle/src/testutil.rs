//! Test utilities for building synthetic bundles in integration tests.
//! This module is only available with the "testutil" feature.

#[cfg(feature = "testutil")]
use crate::hash::murmur_hash64;
#[cfg(feature = "testutil")]
use crate::oodle::Oodle;
#[cfg(feature = "testutil")]
use crate::Result;
#[cfg(feature = "testutil")]
use std::fs::File;
#[cfg(feature = "testutil")]
use std::io::Write;
#[cfg(feature = "testutil")]
use std::path::Path;

/// Entry in a synthetic bundle.
#[cfg(feature = "testutil")]
pub struct SyntheticEntry {
    /// File extension (e.g., "lua", "texture").
    pub ext: String,
    /// File name/content for hashing.
    pub name: String,
    /// Body data content.
    pub body: Vec<u8>,
}

/// Write a synthetic bundle to the given path.
/// This builds a valid single-chunk Oodle-compressed bundle with the provided entries.
///
/// # Arguments
/// * `path` - Where to write the bundle file
/// * `entries` - List of file entries to include in the bundle
/// * `oodle` - Loaded Oodle library for compression
///
/// # Panics
/// Panics if the path cannot be written or Oodle compression fails.
#[cfg(feature = "testutil")]
pub fn write_synthetic_bundle(
    path: &Path,
    entries: &[SyntheticEntry],
    oodle: &Oodle,
) -> Result<()> {
    const CHUNK_SIZE: usize = 0x80000; // 512 KiB

    let mut index_entries = Vec::new();
    let mut file_entries_data = Vec::new();

    for entry in entries {
        let ext_hash = murmur_hash64(entry.ext.as_bytes());
        let name_hash = murmur_hash64(entry.name.as_bytes());

        // Build the FileEntry payload
        // Layout: ext_hash u64, name_hash u64, num_variants u32, flags [u8;4],
        //   variant 0 header (14 bytes): kind u32, unknown1 u8, body_size u32, unknown2 u8, tail_size u32,
        //   body content (body_size bytes), no tail.
        let header_size = 8 + 8 + 4 + 4 + 14; // 38 bytes for headers
        let body_size = entry.body.len();

        let mut file_entry = Vec::with_capacity(header_size + body_size);
        file_entry.extend_from_slice(&ext_hash.to_le_bytes());
        file_entry.extend_from_slice(&name_hash.to_le_bytes());
        file_entry.extend_from_slice(&1u32.to_le_bytes()); // num_variants
        file_entry.extend_from_slice(&[0u8; 4]); // flags
        file_entry.extend_from_slice(&0u32.to_le_bytes()); // variant kind
        file_entry.push(0); // variant unknown1
        file_entry.extend_from_slice(&(body_size as u32).to_le_bytes()); // body_size
        file_entry.push(1); // variant unknown2
        file_entry.extend_from_slice(&0u32.to_le_bytes()); // tail_size

        file_entry.extend_from_slice(&entry.body);

        index_entries.push((ext_hash, name_hash, 0u32));
        file_entries_data.extend_from_slice(&file_entry);
    }

    // Pad to CHUNK_SIZE if needed (bundle parser requires chunks to decompress to exactly CHUNK_SIZE)
    let total_size: u32 = file_entries_data.len() as u32;
    if file_entries_data.len() < CHUNK_SIZE {
        file_entries_data.resize(CHUNK_SIZE, 0);
    } else if file_entries_data.len() > CHUNK_SIZE {
        // If data exceeds CHUNK_SIZE, we'd need multiple chunks, but for simplicity
        // we'll just truncate (this shouldn't happen for small test entries)
        return Err(crate::Error::InvalidBundle(
            "Test data too large for single chunk".to_string(),
        ));
    }

    // Compress the padded data. Since data is now exactly CHUNK_SIZE and padded,
    // compression is safe and produces a smaller payload.
    let mut compressed = vec![0u8; CHUNK_SIZE + (CHUNK_SIZE / 2) + 1024];
    let n = oodle.compress(&file_entries_data, &mut compressed);
    assert!(n > 0, "Oodle compress failed for synthetic bundle");
    assert!(
        n < CHUNK_SIZE,
        "Compressed size must be smaller than CHUNK_SIZE to trigger decompress path"
    );
    compressed.truncate(n);

    // Build the .bundle file
    let mut bundle_data = Vec::with_capacity(4096);

    // Header (12 bytes): magic u64 LE, num_files u32 LE
    bundle_data.extend_from_slice(&0x0000_0003_f000_0008u64.to_le_bytes());
    bundle_data.extend_from_slice(&(entries.len() as u32).to_le_bytes());

    // TypeData (256 bytes)
    bundle_data.extend_from_slice(&[0u8; 256]);

    // Index entries (20 bytes each)
    for (ext_hash, name_hash, mode) in &index_entries {
        bundle_data.extend_from_slice(&ext_hash.to_le_bytes());
        bundle_data.extend_from_slice(&name_hash.to_le_bytes());
        bundle_data.extend_from_slice(&mode.to_le_bytes());
    }

    // Chunk stream header
    bundle_data.extend_from_slice(&1u32.to_le_bytes()); // num_chunks = 1
    bundle_data.extend_from_slice(&(compressed.len() as u32).to_le_bytes()); // chunk size array

    // Alignment padding
    let pos = bundle_data.len() as u64;
    let padding = ((16 - (pos % 16)) % 16) as usize;
    bundle_data.extend_from_slice(&vec![0u8; padding]);

    // total_size and zero
    bundle_data.extend_from_slice(&total_size.to_le_bytes());
    bundle_data.extend_from_slice(&0u32.to_le_bytes());

    // The chunk itself: 4-byte size header, alignment padding, then compressed bytes
    bundle_data.extend_from_slice(&(compressed.len() as u32).to_le_bytes());

    let pos = bundle_data.len() as u64;
    let padding = ((16 - (pos % 16)) % 16) as usize;
    bundle_data.extend_from_slice(&vec![0u8; padding]);

    bundle_data.extend_from_slice(&compressed);

    // Write to file
    let path_str = path.to_str().ok_or_else(|| {
        crate::Error::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Path is not valid UTF-8",
        ))
    })?;
    let mut f = File::create(path_str)?;
    f.write_all(&bundle_data)?;

    Ok(())
}
