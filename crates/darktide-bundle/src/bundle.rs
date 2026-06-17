use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

use crate::error::{Error, Result};
use crate::oodle::Oodle;
use crate::types::{FileEntry, FileVariant, IndexEntry};

/// Classification result for a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileClass {
    /// File is a valid Darktide bundle.
    Bundle,
    /// File exists but is not a bundle.
    NotBundle,
    /// File could not be read (IO error).
    Unreadable,
}

/// Darktide resource bundle.
#[derive(Debug)]
pub struct Bundle {
    file: File,
    num_files: u32,
    num_chunks: u32,
    total_size: u32,
    chunk_data_offset: u64,
}

impl Bundle {
    /// Classify a file by checking its magic bytes.
    /// Reads only the first 8 bytes, so it's fast and doesn't parse the whole bundle.
    pub fn classify(path: &str) -> FileClass {
        let mut file = match File::open(path) {
            Ok(f) => f,
            Err(_) => return FileClass::Unreadable,
        };

        let mut header = [0u8; 8];
        match file.read_exact(&mut header) {
            Ok(_) => {}
            Err(_) => return FileClass::Unreadable,
        }

        let magic = u64::from_le_bytes(header);
        if magic == 0x0000_0003_f000_0008 || magic == 0x0000_0003_f000_0007 {
            FileClass::Bundle
        } else {
            FileClass::NotBundle
        }
    }

    /// Check if a file is a valid bundle.
    pub fn is_bundle(path: &str) -> bool {
        matches!(Self::classify(path), FileClass::Bundle)
    }

    /// Open a bundle file from disk.
    pub fn open(path: &str) -> Result<Self> {
        let mut file = File::open(path)?;
        let file_size = file.metadata()?.len();

        // Read header (12 bytes)
        let mut header = [0u8; 12];
        file.read_exact(&mut header)?;

        // Validate magic
        let magic = u64::from_le_bytes(header[..8].try_into().unwrap());
        if magic != 0x00000003f0000008 && magic != 0x00000003f0000007 {
            return Err(Error::InvalidBundle(format!(
                "Invalid bundle magic: 0x{:016x}",
                magic
            )));
        }

        let num_files = u32::from_le_bytes(header[8..12].try_into().unwrap());

        // M1: Check num_files doesn't exceed file size (20 bytes per index entry)
        if (num_files as u64) * 20 > file_size {
            return Err(Error::InvalidBundle(format!(
                "num_files {} exceeds file size",
                num_files
            )));
        }

        // Skip TypeData (256 bytes)
        file.seek(SeekFrom::Current(256))?;

        // Skip file index (20 bytes per entry)
        file.seek(SeekFrom::Current(num_files as i64 * 20))?;

        // Read num_chunks
        let mut buf4 = [0u8; 4];
        file.read_exact(&mut buf4)?;
        let num_chunks = u32::from_le_bytes(buf4);

        // M1: Check num_chunks doesn't exceed file size (4 bytes per chunk size)
        if (num_chunks as u64) * 4 > file_size {
            return Err(Error::InvalidBundle(format!(
                "num_chunks {} exceeds file size",
                num_chunks
            )));
        }

        // M4: Skip chunk size array (don't store it)
        file.seek(SeekFrom::Current(num_chunks as i64 * 4))?;

        // Align to 16 bytes (based on actual file position)
        let pos = file.stream_position()?;
        file.seek(SeekFrom::Current(((16 - (pos % 16)) % 16) as i64))?;

        // Read total_size and zero
        file.read_exact(&mut buf4)?;
        let total_size = u32::from_le_bytes(buf4);
        file.read_exact(&mut buf4)?; // zero, ignore

        let chunk_data_offset = file.stream_position()?;

        Ok(Bundle {
            file,
            num_files,
            num_chunks,
            total_size,
            chunk_data_offset,
        })
    }

    /// Read the file index (20 bytes per entry).
    pub fn read_index(&mut self) -> Result<Vec<IndexEntry>> {
        self.file.seek(SeekFrom::Start(12 + 256))?;
        let mut entries = Vec::with_capacity(self.num_files as usize);
        for _ in 0..self.num_files {
            let mut buf8 = [0u8; 8];
            let mut buf4 = [0u8; 4];
            self.file.read_exact(&mut buf8)?;
            let ext = u64::from_le_bytes(buf8);
            self.file.read_exact(&mut buf8)?;
            let name = u64::from_le_bytes(buf8);
            self.file.read_exact(&mut buf4)?;
            let mode = u32::from_le_bytes(buf4);
            entries.push(IndexEntry { ext, name, mode });
        }
        Ok(entries)
    }

    /// Decompress all chunks and return the raw decompressed buffer.
    pub fn decompress_chunks(&mut self, oodle: &Oodle) -> Result<Vec<u8>> {
        const CHUNK_SIZE: usize = 0x80000; // 512KB
        let scratch_size = CHUNK_SIZE * 3;
        let scratch = vec![0u8; scratch_size];

        self.file.seek(SeekFrom::Start(self.chunk_data_offset))?;

        // M1: Bound capacity by num_chunks * CHUNK_SIZE
        let capacity = (self.total_size as usize).min((self.num_chunks as usize) * CHUNK_SIZE);
        let mut decompressed = Vec::with_capacity(capacity);

        for _ in 0..self.num_chunks {
            // Read chunk size from the per-chunk header (4 bytes)
            let mut sz = [0u8; 4];
            self.file.read_exact(&mut sz)?;
            let chunk_size = u32::from_le_bytes(sz) as usize;

            // 16-byte alignment padding
            let pos = self.file.stream_position()?;
            let padding = ((16 - (pos % 16)) % 16) as i64;
            if padding > 0 {
                self.file.seek(SeekFrom::Current(padding))?;
            }

            let mut compressed = vec![0u8; chunk_size];
            self.file.read_exact(&mut compressed)?;

            // L9: A chunk equal to the block size signals stored-uncompressed;
            // smaller means Oodle-compressed. (Format convention, validated across all bundles.)
            if chunk_size == CHUNK_SIZE {
                // Stored uncompressed
                decompressed.extend_from_slice(&compressed);
            } else {
                // Oodle-compressed
                let mut output = vec![0u8; CHUNK_SIZE];
                let result = oodle.decompress(&compressed, &mut output, &scratch);
                if result == 0 {
                    return Err(Error::OodleDecompress);
                }
                decompressed.extend_from_slice(&output);
            }
        }

        Ok(decompressed)
    }

    /// Extract all files from the decompressed stream.
    pub fn parse_files(decompressed: &[u8]) -> Result<Vec<FileEntry>> {
        let mut pos: usize = 0;
        let mut files = Vec::new();

        while pos < decompressed.len() {
            // Read file header
            if pos + 8 + 8 + 4 + 4 > decompressed.len() {
                break;
            }

            let ext = u64::from_le_bytes(decompressed[pos..pos + 8].try_into().unwrap());
            pos += 8;
            let name = u64::from_le_bytes(decompressed[pos..pos + 8].try_into().unwrap());
            pos += 8;
            let num_variants = u32::from_le_bytes(decompressed[pos..pos + 4].try_into().unwrap());
            pos += 4;
            let flags = decompressed[pos..pos + 4].try_into().unwrap();
            pos += 4;

            // M1: Check num_variants doesn't exceed remaining buffer (14 bytes minimum per variant)
            if pos > decompressed.len() {
                return Err(Error::InvalidBundle("Invalid file position".into()));
            }
            let remaining = decompressed.len() - pos;
            if (num_variants as usize) * 14 > remaining {
                return Err(Error::InvalidBundle(format!(
                    "num_variants {} exceeds remaining buffer size {}",
                    num_variants, remaining
                )));
            }

            let mut variants = Vec::with_capacity(num_variants as usize);
            let mut content_size: usize = 0;

            for _ in 0..num_variants {
                if pos + 4 + 1 + 4 + 1 + 4 > decompressed.len() {
                    return Err(Error::InvalidBundle("Truncated variant data".into()));
                }
                let kind = u32::from_le_bytes(decompressed[pos..pos + 4].try_into().unwrap());
                pos += 4;
                let unknown1 = decompressed[pos];
                pos += 1;
                let body_size = u32::from_le_bytes(decompressed[pos..pos + 4].try_into().unwrap());
                pos += 4;
                let unknown2 = decompressed[pos];
                pos += 1;
                let tail_size = u32::from_le_bytes(decompressed[pos..pos + 4].try_into().unwrap());
                pos += 4;

                variants.push(FileVariant {
                    kind,
                    unknown1,
                    body_size,
                    unknown2,
                    tail_size,
                });
                // L8: Use saturating_add for 32-bit safety
                content_size = content_size
                    .saturating_add((body_size as usize).saturating_add(tail_size as usize));
            }

            if pos + content_size > decompressed.len() {
                return Err(Error::InvalidBundle(
                    "File content exceeds decompressed data".into(),
                ));
            }

            let data = decompressed[pos..pos + content_size].to_vec();
            pos += content_size;

            files.push(FileEntry {
                ext,
                name,
                num_variants,
                flags,
                variants,
                data,
            });
        }

        Ok(files)
    }

    /// Full extraction: decompress chunks and parse files.
    pub fn extract_files(&mut self, oodle: &Oodle) -> Result<Vec<FileEntry>> {
        let decompressed = self.decompress_chunks(oodle)?;
        // Only parse up to total_size — decompressed buffer may have trailing padding
        let end = (self.total_size as usize).min(decompressed.len());
        let data = &decompressed[..end];
        Self::parse_files(data)
    }
}
