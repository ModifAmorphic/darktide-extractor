//! C ABI exports for Darktide bundle extraction.
//!
//! # Functions
//!
//! - `darktide_oodle_load(path)` — Load the Oodle shared library
//! - `darktide_oodle_free(oodle)` — Free an Oodle handle
//! - `darktide_bundle_open(path)` — Open a bundle file
//! - `darktide_bundle_free(bundle)` — Free a bundle handle
//! - `darktide_bundle_file_count(bundle)` — Get number of files in the index
//! - `darktide_bundle_read_index(bundle)` — Read the file index
//! - `darktide_bundle_index_entry(bundle, idx, out)` — Get an index entry
//! - `darktide_bundle_index_free(index)` — Free an index
//! - `darktide_bundle_extract_all(bundle, oodle)` — Extract all files
//! - `darktide_bundle_files_count(files)` — Get number of extracted files
//! - `darktide_bundle_file_entry(files, idx, out)` — Get file metadata
//! - `darktide_bundle_file_data(files, idx, out_buf, out_len)` — Copy file data
//! - `darktide_bundle_files_free(files)` — Free extracted files
//! - `darktide_murmur_hash64(data, len)` — Compute MurmurHash64A
//! - `darktide_lookup_extension(hash)` — Lookup extension name by hash

use darktide_bundle::hash::compute_known_extensions;
use darktide_bundle::{Bundle, FileEntry, IndexEntry, Oodle};
use std::collections::HashMap;
use std::ffi::{c_char, c_int, c_uint, CStr, CString};
use std::ptr;
use std::sync::OnceLock;

// ---------------------------------------------------------------------------
// Opaque handle types
// ---------------------------------------------------------------------------

/// Opaque handle to a loaded Oodle library.
pub struct DarktideOodle {
    inner: Oodle,
}

/// Opaque handle to an opened bundle.
pub struct DarktideBundle {
    inner: Bundle,
}

/// Opaque handle to a file index.
pub struct DarktideIndex {
    inner: Vec<IndexEntry>,
}

/// Opaque handle to extracted files.
pub struct DarktideFiles {
    inner: Vec<FileEntry>,
}

// ---------------------------------------------------------------------------
// C-compatible structs
// ---------------------------------------------------------------------------

/// C-compatible index entry.
#[repr(C)]
pub struct DarktideIndexEntry {
    pub ext: u64,
    pub name: u64,
    pub mode: u32,
}

/// C-compatible file entry metadata.
#[repr(C)]
pub struct DarktideFileEntry {
    pub ext: u64,
    pub name: u64,
    pub num_variants: u32,
    pub data_len: u64,
}

// ---------------------------------------------------------------------------
// Error handling convention:
//   - Return pointer on success, null on failure
//   - Return 0 on success, -1 on failure (for functions returning int)
//   - Return count/size on success, -1 (cast) on failure
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Oodle
// ---------------------------------------------------------------------------

/// Load the Oodle shared library.
/// Returns a handle on success, null on failure.
///
/// # Safety
///
/// - `path` must be a valid pointer to a NUL-terminated C string.
#[no_mangle]
pub unsafe extern "C" fn darktide_oodle_load(path: *const c_char) -> *mut DarktideOodle {
    if path.is_null() {
        return ptr::null_mut();
    }
    let path_str = match unsafe { CStr::from_ptr(path) }.to_str() {
        Ok(s) => s,
        Err(_) => return ptr::null_mut(),
    };
    match Oodle::load(path_str) {
        Ok(oodle) => Box::into_raw(Box::new(DarktideOodle { inner: oodle })),
        Err(_) => ptr::null_mut(),
    }
}

/// Free an Oodle handle.
///
/// # Safety
///
/// - `oodle` must be a pointer returned by `darktide_oodle_load`, or null.
/// - Must be called at most once per handle (double-free is UB).
#[no_mangle]
pub unsafe extern "C" fn darktide_oodle_free(oodle: *mut DarktideOodle) {
    if !oodle.is_null() {
        unsafe { drop(Box::from_raw(oodle)) };
    }
}

// ---------------------------------------------------------------------------
// Bundle
// ---------------------------------------------------------------------------

/// Open a bundle file.
/// Returns a handle on success, null on failure.
///
/// # Safety
///
/// - `path` must be a valid pointer to a NUL-terminated C string.
#[no_mangle]
pub unsafe extern "C" fn darktide_bundle_open(path: *const c_char) -> *mut DarktideBundle {
    if path.is_null() {
        return ptr::null_mut();
    }
    let path_str = match unsafe { CStr::from_ptr(path) }.to_str() {
        Ok(s) => s,
        Err(_) => return ptr::null_mut(),
    };
    match Bundle::open(path_str) {
        Ok(bundle) => Box::into_raw(Box::new(DarktideBundle { inner: bundle })),
        Err(_) => ptr::null_mut(),
    }
}

/// Free a bundle handle.
///
/// # Safety
///
/// - `bundle` must be a pointer returned by `darktide_bundle_open`, or null.
/// - Must be called at most once per handle (double-free is UB).
#[no_mangle]
pub unsafe extern "C" fn darktide_bundle_free(bundle: *mut DarktideBundle) {
    if !bundle.is_null() {
        unsafe { drop(Box::from_raw(bundle)) };
    }
}

// ---------------------------------------------------------------------------
// Index
// ---------------------------------------------------------------------------

/// Read the file index from a bundle.
/// Returns a handle on success, null on failure.
///
/// # Safety
///
/// - `bundle` must be a pointer returned by `darktide_bundle_open`, or null.
/// - The caller must guarantee exclusive access (no concurrent calls on the same handle).
#[no_mangle]
pub unsafe extern "C" fn darktide_bundle_read_index(
    bundle: *mut DarktideBundle,
) -> *mut DarktideIndex {
    if bundle.is_null() {
        return ptr::null_mut();
    }
    let bundle = unsafe { &mut *bundle };
    match bundle.inner.read_index() {
        Ok(index) => Box::into_raw(Box::new(DarktideIndex { inner: index })),
        Err(_) => ptr::null_mut(),
    }
}

/// Get the number of entries in an index.
///
/// # Safety
///
/// - `index` must be a pointer returned by `darktide_bundle_read_index`, or null.
#[no_mangle]
pub unsafe extern "C" fn darktide_bundle_index_count(index: *const DarktideIndex) -> c_uint {
    if index.is_null() {
        return 0;
    }
    unsafe { (*index).inner.len() as c_uint }
}

/// Get an index entry by index.
/// Returns 0 on success, -1 on failure.
///
/// # Safety
///
/// - `index` must be a pointer returned by `darktide_bundle_read_index`, or null.
/// - `out` must be a valid pointer to a `DarktideIndexEntry`.
#[no_mangle]
pub unsafe extern "C" fn darktide_bundle_index_entry(
    index: *const DarktideIndex,
    idx: c_uint,
    out: *mut DarktideIndexEntry,
) -> c_int {
    if index.is_null() || out.is_null() {
        return -1;
    }
    let index = unsafe { &*index };
    match index.inner.get(idx as usize) {
        Some(entry) => {
            let out = unsafe { &mut *out };
            out.ext = entry.ext;
            out.name = entry.name;
            out.mode = entry.mode;
            0
        }
        None => -1,
    }
}

/// Free an index handle.
///
/// # Safety
///
/// - `index` must be a pointer returned by `darktide_bundle_read_index`, or null.
/// - Must be called at most once per handle (double-free is UB).
#[no_mangle]
pub unsafe extern "C" fn darktide_bundle_index_free(index: *mut DarktideIndex) {
    if !index.is_null() {
        unsafe { drop(Box::from_raw(index)) };
    }
}

// ---------------------------------------------------------------------------
// Extraction
// ---------------------------------------------------------------------------

/// Extract all files from a bundle using the given Oodle handle.
/// Returns a handle on success, null on failure.
///
/// # Safety
///
/// - `bundle` must be a pointer returned by `darktide_bundle_open`, or null.
/// - `oodle` must be a pointer returned by `darktide_oodle_load`, or null.
/// - The caller must guarantee exclusive access to both handles (no concurrent calls).
#[no_mangle]
pub unsafe extern "C" fn darktide_bundle_extract_all(
    bundle: *mut DarktideBundle,
    oodle: *mut DarktideOodle,
) -> *mut DarktideFiles {
    if bundle.is_null() || oodle.is_null() {
        return ptr::null_mut();
    }
    let bundle = unsafe { &mut *bundle };
    let oodle = unsafe { &(*oodle).inner };
    match bundle.inner.extract_files(oodle) {
        Ok(files) => Box::into_raw(Box::new(DarktideFiles { inner: files })),
        Err(_) => ptr::null_mut(),
    }
}

/// Get the number of extracted files.
///
/// # Safety
///
/// - `files` must be a pointer returned by `darktide_bundle_extract_all`, or null.
#[no_mangle]
pub unsafe extern "C" fn darktide_bundle_files_count(files: *const DarktideFiles) -> c_uint {
    if files.is_null() {
        return 0;
    }
    unsafe { (*files).inner.len() as c_uint }
}

/// Get metadata for an extracted file by index.
/// Returns 0 on success, -1 on failure.
///
/// # Safety
///
/// - `files` must be a pointer returned by `darktide_bundle_extract_all`, or null.
/// - `out` must be a valid pointer to a `DarktideFileEntry`.
#[no_mangle]
pub unsafe extern "C" fn darktide_bundle_file_entry(
    files: *const DarktideFiles,
    idx: c_uint,
    out: *mut DarktideFileEntry,
) -> c_int {
    if files.is_null() || out.is_null() {
        return -1;
    }
    let files = unsafe { &*files };
    match files.inner.get(idx as usize) {
        Some(file) => {
            let out = unsafe { &mut *out };
            out.ext = file.ext;
            out.name = file.name;
            out.num_variants = file.num_variants;
            out.data_len = file.data.len() as u64;
            0
        }
        None => -1,
    }
}

/// Copy file data for an extracted file by index into the provided buffer.
/// Returns the number of bytes copied, or -1 on failure.
///
/// # Safety
///
/// - `files` must be a pointer returned by `darktide_bundle_extract_all`, or null.
/// - `out_buf` must be a valid pointer to a buffer of at least `out_len` bytes.
#[no_mangle]
pub unsafe extern "C" fn darktide_bundle_file_data(
    files: *const DarktideFiles,
    idx: c_uint,
    out_buf: *mut u8,
    out_len: u64,
) -> i64 {
    if files.is_null() || out_buf.is_null() {
        return -1;
    }
    let files = unsafe { &*files };
    match files.inner.get(idx as usize) {
        Some(file) => {
            let copy_len = (file.data.len() as u64).min(out_len) as usize;
            unsafe { ptr::copy_nonoverlapping(file.data.as_ptr(), out_buf, copy_len) };
            copy_len as i64
        }
        None => -1,
    }
}

/// Free extracted files handle.
///
/// # Safety
///
/// - `files` must be a pointer returned by `darktide_bundle_extract_all`, or null.
/// - Must be called at most once per handle (double-free is UB).
#[no_mangle]
pub unsafe extern "C" fn darktide_bundle_files_free(files: *mut DarktideFiles) {
    if !files.is_null() {
        unsafe { drop(Box::from_raw(files)) };
    }
}

// ---------------------------------------------------------------------------
// Hash utilities
// ---------------------------------------------------------------------------

static EXTENSION_CSTRINGS: OnceLock<HashMap<u64, CString>> = OnceLock::new();

fn extension_cstrings() -> &'static HashMap<u64, CString> {
    EXTENSION_CSTRINGS.get_or_init(|| {
        compute_known_extensions()
            .into_iter()
            .map(|(name, hash)| {
                // SAFE: all known extensions are ASCII with no interior NUL.
                let cstr = CString::new(name).expect("extension name has no NUL");
                (hash, cstr)
            })
            .collect()
    })
}

/// Compute MurmurHash64A of the given data.
///
/// # Safety
///
/// - `data` must be a valid pointer to a buffer of `len` bytes, or null.
/// - If `data` is null, the function returns 0.
#[no_mangle]
pub unsafe extern "C" fn darktide_murmur_hash64(data: *const u8, len: c_uint) -> u64 {
    if data.is_null() || len == 0 {
        return 0;
    }
    let slice = unsafe { std::slice::from_raw_parts(data, len as usize) };
    darktide_bundle::hash::murmur_hash64(slice)
}

/// Lookup extension name by hash.
/// Returns a pointer to a NUL-terminated C string valid for the lifetime of the program,
/// or null if unknown.
#[no_mangle]
pub extern "C" fn darktide_lookup_extension(hash: u64) -> *const c_char {
    extension_cstrings()
        .get(&hash)
        .map(|s| s.as_ptr())
        .unwrap_or(ptr::null())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_darktide_lookup_extension_nul_terminated() {
        let lua_hash = darktide_bundle::hash::murmur_hash64(b"lua");
        let ptr = darktide_lookup_extension(lua_hash);
        assert!(!ptr.is_null());

        // Verify the byte after the string is 0x00
        let cstr = unsafe { CStr::from_ptr(ptr) };
        let bytes = cstr.to_bytes();
        assert_eq!(bytes, b"lua");
    }

    #[test]
    fn test_darktide_lookup_extension_unknown() {
        let unknown_hash = 0xdeadbeef_deadbeef;
        let ptr = darktide_lookup_extension(unknown_hash);
        assert!(ptr.is_null());
    }
}
