use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};

use crate::oodle::Oodle;
use crate::types::{FileEntry, FileVariant, IndexEntry};

/// Darktide resource bundle.
pub struct Bundle {
    file: File,
    num_files: u32,
    chunk_sizes: Vec<u32>,
    total_size: u32,
    chunk_data_offset: u64,
}

impl Bundle {
    /// Open a bundle file from disk.
    pub fn open(path: &str) -> io::Result<Self> {
        let mut file = File::open(path)?;

        // Read header (12 bytes)
        let mut header = [0u8; 12];
        file.read_exact(&mut header)?;

        // Validate magic
        let magic = u64::from_le_bytes(header[..8].try_into().unwrap());
        if magic != 0x00000003f0000008 && magic != 0x00000003f0000007 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Invalid bundle magic: 0x{:016x}", magic),
            ));
        }

        let num_files = u32::from_le_bytes(header[8..12].try_into().unwrap());

        // Skip TypeData (256 bytes)
        file.seek(SeekFrom::Current(256))?;

        // Skip file index (20 bytes per entry)
        file.seek(SeekFrom::Current(num_files as i64 * 20))?;

        // Read num_chunks
        let mut buf4 = [0u8; 4];
        file.read_exact(&mut buf4)?;
        let num_chunks = u32::from_le_bytes(buf4);

        // Read chunk sizes
        let mut chunk_sizes = Vec::with_capacity(num_chunks as usize);
        for _ in 0..num_chunks {
            file.read_exact(&mut buf4)?;
            chunk_sizes.push(u32::from_le_bytes(buf4));
        }

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
            chunk_sizes,
            total_size,
            chunk_data_offset,
        })
    }

    /// Read the file index (20 bytes per entry).
    pub fn read_index(&mut self) -> io::Result<Vec<IndexEntry>> {
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
    pub fn decompress_chunks(&mut self, oodle: &Oodle) -> io::Result<Vec<u8>> {
        const CHUNK_SIZE: usize = 0x80000; // 512KB
        let scratch_size = CHUNK_SIZE * 3;
        let scratch = vec![0u8; scratch_size];

        self.file.seek(SeekFrom::Start(self.chunk_data_offset))?;

        let mut decompressed = Vec::with_capacity(self.total_size as usize);

        for _ in 0..self.chunk_sizes.len() {
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
            self.file.read_exact(&mut compressed).map_err(|e| {
                io::Error::new(
                    e.kind(),
                    format!(
                        "Failed to read chunk data (chunk_size={}, file_pos={}): {}",
                        chunk_size,
                        self.file.stream_position().unwrap_or(0),
                        e
                    ),
                )
            })?;

            if chunk_size == CHUNK_SIZE {
                // Stored uncompressed
                decompressed.extend_from_slice(&compressed);
            } else {
                // Oodle-compressed
                let mut output = vec![0u8; CHUNK_SIZE];
                let result = oodle.decompress(&compressed, &mut output, &scratch);
                if result == 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Oodle decompression failed",
                    ));
                }
                decompressed.extend_from_slice(&output);
            }
        }

        Ok(decompressed)
    }

    /// Extract all files from the decompressed stream.
    pub fn parse_files(decompressed: &[u8]) -> io::Result<Vec<FileEntry>> {
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

            let mut variants = Vec::with_capacity(num_variants as usize);
            let mut content_size: usize = 0;

            for _ in 0..num_variants {
                if pos + 4 + 1 + 4 + 1 + 4 > decompressed.len() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Truncated variant data",
                    ));
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
                content_size += body_size as usize + tail_size as usize;
            }

            if pos + content_size > decompressed.len() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "File content exceeds decompressed data",
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
    pub fn extract_files(&mut self, oodle: &Oodle) -> io::Result<Vec<FileEntry>> {
        let decompressed = self.decompress_chunks(oodle)?;
        // Only parse up to total_size — decompressed buffer may have trailing padding
        let end = (self.total_size as usize).min(decompressed.len());
        let data = &decompressed[..end];
        Self::parse_files(data)
    }
}
