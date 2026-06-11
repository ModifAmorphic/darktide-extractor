// Core types for Darktide bundle format

/// Entry in the bundle's file index (on-disk, before decompression).
#[derive(Debug, Clone)]
pub struct IndexEntry {
    /// MurmurHash64 of the file extension (e.g. "lua", "texture").
    pub ext: u64,
    /// MurmurHash64 of the file name.
    pub name: u64,
    /// Unknown flags.
    pub mode: u32,
}

/// A variant of a file (files can have multiple variants).
#[derive(Debug, Clone)]
pub struct FileVariant {
    /// Kind of variant.
    pub kind: u32,
    /// Unknown, 0 or 1.
    pub unknown1: u8,
    /// Size of the body data.
    pub body_size: u32,
    /// Always 1.
    pub unknown2: u8,
    /// Size of the tail data.
    pub tail_size: u32,
}

/// A file extracted from the decompressed bundle stream.
#[derive(Debug, Clone)]
pub struct FileEntry {
    /// MurmurHash64 of the file extension.
    pub ext: u64,
    /// MurmurHash64 of the file name.
    pub name: u64,
    /// Number of variants.
    pub num_variants: u32,
    /// Flags (must be 0).
    pub flags: [u8; 4],
    /// Variants for this file.
    pub variants: Vec<FileVariant>,
    /// Raw file content (body + tail for all variants concatenated).
    pub data: Vec<u8>,
}
