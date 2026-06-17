use libloading::Library;
use std::ffi::c_void;
use std::os::raw::c_int;
use std::path::PathBuf;

use crate::error::{Error, Result};

/// OodleLZ_Decompress FFI signature (cross-platform, 14 args).
/// Reverse-engineered signature for Oodle 2.9.14 (Kraken); verified empirically against
/// 9,648 Darktide bundles. Sizes use `usize` to match the library's `size_t` ABI.
#[allow(non_camel_case_types)]
type OodleLZ_DecompressFn = unsafe extern "C" fn(
    src: *const u8, // compressed data
    src_size: usize,
    dst: *mut u8,                // output buffer (must be 0x80000)
    dst_size: usize,             // 0x80000
    fuzz_safe: c_int,            // 1
    check_crc: c_int,            // 0
    verbose: c_int,              // 0 = silent, 3 = verbose debug
    dst_log2s: *mut u8,          // null
    decoder_mem_size: usize,     // 0
    decoder_mem: *mut c_void,    // null
    callback: *mut c_void,       // null
    callback_user_data: *mut u8, // null
    scratch: usize,              // scratch buffer size
    demand_continue: c_int,      // 3
) -> usize;

/// OodleLZ_Compress FFI signature (public, documented).
/// Constants used: OodleLZ_Compressor_Kraken = 8, OodleLZ_CompressionLevel_Normal = 4.
/// Sizes use `isize` to match SINTa (ptrdiff_t) in the official header.
#[allow(non_camel_case_types)]
type OodleLZ_CompressFn = unsafe extern "C" fn(
    compressor: c_int, // 8 = Kraken
    src: *const u8,
    src_size: isize,
    dst: *mut u8,
    level: c_int,            // 4 = Normal
    opts: *mut c_void,       // null
    dictionary_base: isize,  // 0
    dictionary: *mut c_void, // null
    dictionary_size: isize,  // 0
) -> isize;

/// Loaded Oodle library handle.
pub struct Oodle {
    _lib: Library,
    decompress: libloading::Symbol<'static, OodleLZ_DecompressFn>,
    compress: libloading::Symbol<'static, OodleLZ_CompressFn>,
    verbose: bool,
}

impl Oodle {
    /// Load the Oodle shared library from the given path (silent mode).
    pub fn load(path: &str) -> Result<Self> {
        Self::load_verbose(path, false)
    }

    /// Load the Oodle shared library from the given path with explicit verbose flag.
    pub fn load_verbose(path: &str, verbose: bool) -> Result<Self> {
        let lib =
            unsafe { Library::new(path) }.map_err(|e| Error::OodleLoad(format!("{path}: {e}")))?;
        let decompress = unsafe { lib.get(b"OodleLZ_Decompress") }.map_err(|e| {
            Error::OodleLoad(format!("{path}: symbol OodleLZ_Decompress not found: {e}"))
        })?;
        // Safety: The Symbol lives as long as the Library, which is stored in self.
        let decompress = unsafe {
            std::mem::transmute::<
                libloading::Symbol<'_, OodleLZ_DecompressFn>,
                libloading::Symbol<'static, OodleLZ_DecompressFn>,
            >(decompress)
        };
        let compress = unsafe { lib.get(b"OodleLZ_Compress") }.map_err(|e| {
            Error::OodleLoad(format!("{path}: symbol OodleLZ_Compress not found: {e}"))
        })?;
        let compress = unsafe {
            std::mem::transmute::<
                libloading::Symbol<'_, OodleLZ_CompressFn>,
                libloading::Symbol<'static, OodleLZ_CompressFn>,
            >(compress)
        };
        Ok(Oodle {
            _lib: lib,
            decompress,
            compress,
            verbose,
        })
    }

    /// Generate candidate library paths for Oodle discovery.
    /// Returns paths in order: <exe-dir>/<name>, <cwd>/<name>, <name> (bare).
    fn oodle_candidates(name: &str) -> Vec<PathBuf> {
        let mut candidates = Vec::new();

        // <exe-dir>/<name>
        if let Ok(exe) = std::env::current_exe() {
            if let Some(exe_dir) = exe.parent() {
                candidates.push(exe_dir.join(name));
            }
        }

        // <cwd>/<name>
        if let Ok(cwd) = std::env::current_dir() {
            candidates.push(cwd.join(name));
        }

        // <name> (bare, let system loader resolve)
        candidates.push(PathBuf::from(name));

        // Deduplicate while preserving order
        let mut seen = std::collections::HashSet::new();
        candidates.retain(|p| seen.insert(p.clone()));

        candidates
    }

    /// Discover and load the Oodle library from common locations (silent mode).
    /// Tries candidate paths in order and returns the first successful load.
    pub fn discover(name: &str) -> Result<Self> {
        Self::discover_verbose(name, false)
    }

    /// Discover and load the Oodle library from candidate paths with explicit verbose flag.
    /// Tries candidates in order and returns the first successful load.
    fn discover_from_candidates(candidates: &[PathBuf], verbose: bool) -> Result<Self> {
        if candidates.is_empty() {
            return Err(Error::OodleLoad("no candidate paths".into()));
        }

        let mut errors = Vec::new();

        for path in candidates {
            let path_str = match path.to_str() {
                Some(s) => s,
                None => {
                    errors.push(format!("{}: path is not valid UTF-8", path.display()));
                    continue;
                }
            };

            match Self::load_verbose(path_str, verbose) {
                Ok(oodle) => return Ok(oodle),
                Err(e) => errors.push(format!("{}: {}", path.display(), e)),
            }
        }

        // All candidates failed
        Err(Error::OodleLoad(format!(
            "tried paths:\n  {}",
            errors.join("\n  ")
        )))
    }

    /// Discover and load the Oodle library from common locations with explicit verbose flag.
    /// Tries candidate paths in order and returns the first successful load.
    pub fn discover_verbose(name: &str, verbose: bool) -> Result<Self> {
        let candidates = Self::oodle_candidates(name);
        Self::discover_from_candidates(&candidates, verbose)
    }

    /// Decompress `src` into `dst` using `scratch` as temporary workspace.
    /// Returns the number of bytes written to `dst`, or 0 on failure.
    pub fn decompress(&self, src: &[u8], dst: &mut [u8], scratch: &[u8]) -> usize {
        let verbose_flag = if self.verbose { 3 } else { 0 };
        unsafe {
            (self.decompress)(
                src.as_ptr(),
                src.len(),
                dst.as_mut_ptr(),
                dst.len(),
                1, // fuzz_safe
                0, // check_crc
                verbose_flag,
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                scratch.len(),
                3, // demand_continue
            )
        }
    }

    /// Compress `src` into `dst` using Kraken at Normal level.
    ///
    /// `dst` must be sized generously — at least `src.len() + (src.len() / 2) + 1024` is a
    /// safe upper bound for compressible input; for incompressible input the output may
    /// exceed `src.len()` slightly. Returns the number of bytes written to `dst`, or 0 on
    /// failure.
    ///
    /// This wraps `OodleLZ_Compress` with `compressor = Kraken (8)` and `level = Normal (4)`.
    pub fn compress(&self, src: &[u8], dst: &mut [u8]) -> usize {
        unsafe {
            (self.compress)(
                8, // Kraken
                src.as_ptr(),
                src.len() as isize,
                dst.as_mut_ptr(),
                4, // Normal
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                0,
            ) as usize
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_oodle_candidates_ordering() {
        let name = "test_lib.so";
        let candidates = Oodle::oodle_candidates(name);

        // Should have at least 3 candidates
        assert!(candidates.len() >= 3);

        // Last candidate should be the bare name
        assert_eq!(candidates.last(), Some(&PathBuf::from(name)));

        // Deduplication: if exe-dir and cwd are the same, candidates should be deduped
        let unique: std::collections::HashSet<_> = candidates.iter().collect();
        assert_eq!(candidates.len(), unique.len());
    }

    #[test]
    fn test_oodle_candidates_dedup() {
        // Mock a scenario where exe-dir and cwd might overlap
        // This test is mostly about verifying the dedup logic
        let name = "test_lib.so";
        let candidates = Oodle::oodle_candidates(name);

        let mut seen = std::collections::HashSet::new();
        for c in &candidates {
            assert!(!seen.contains(c), "Duplicate candidate: {:?}", c);
            seen.insert(c.clone());
        }
    }

    #[test]
    fn discover_returns_first_success() {
        // Build path to the real Oodle lib at repo root
        #[cfg(target_os = "windows")]
        let lib_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../oo2core_9_win64.dll");

        #[cfg(not(target_os = "windows"))]
        let lib_path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../liboo2corelinux64.so.9");

        // The test gates on the lib being present (CI provides it)
        assert!(
            lib_path.exists(),
            "Oodle library not found at {}",
            lib_path.display()
        );

        let result = Oodle::discover_from_candidates(&[lib_path], false);
        assert!(
            result.is_ok(),
            "discover_from_candidates should succeed with valid library"
        );
    }

    #[test]
    fn discover_aggregates_errors_when_all_fail() {
        // Use two bogus non-existent paths
        let bogus1 = PathBuf::from("/nonexistent/path1/libfake.so");
        let bogus2 = PathBuf::from("/nonexistent/path2/libfake.so");

        let result = Oodle::discover_from_candidates(&[bogus1, bogus2], false);
        assert!(
            result.is_err(),
            "discover_from_candidates should fail with invalid paths"
        );

        let err_msg = match result {
            Err(e) => e.to_string(),
            Ok(_) => unreachable!("expected error"),
        };
        // Error should mention both paths
        assert!(
            err_msg.contains("/nonexistent/path1"),
            "Error should mention first bogus path"
        );
        assert!(
            err_msg.contains("/nonexistent/path2"),
            "Error should mention second bogus path"
        );
    }

    #[test]
    fn discover_from_candidates_empty_slice() {
        let result = Oodle::discover_from_candidates(&[], false);
        assert!(result.is_err());
        let err_msg = match result {
            Err(e) => e.to_string(),
            Ok(_) => unreachable!("expected error"),
        };
        assert!(err_msg.contains("no candidate paths"));
    }
}
