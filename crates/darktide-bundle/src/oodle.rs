use libloading::Library;
use std::ffi::c_void;
use std::os::raw::c_int;

/// FFI signature for OodleLZ_Decompress (Linux .so, 14 args).
/// Based on the working example from PLAN.md.
#[allow(non_camel_case_types)]
type OodleLZ_DecompressFn = unsafe extern "C" fn(
    src: *const u8,           // compressed data
    src_size: u64,
    dst: *mut u8,             // output buffer (must be 0x80000)
    dst_size: u64,            // 0x80000
    fuzz_safe: c_int,         // 1
    check_crc: c_int,         // 0
    verbose: c_int,           // 3
    dst_log2s: *mut u8,       // null
    decoder_mem_size: u64,    // 0
    decoder_mem: *mut c_void, // null
    callback: *mut c_void,    // null
    callback_user_data: *mut u8, // null
    scratch: u64,             // scratch buffer size
    demand_continue: c_int,   // 3
) -> u64;

/// Loaded Oodle library handle.
pub struct Oodle {
    _lib: Library,
    decompress: libloading::Symbol<'static, OodleLZ_DecompressFn>,
}

impl Oodle {
    /// Load the Oodle shared library from the given path.
    pub fn load(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let lib = unsafe { Library::new(path)? };
        let decompress: libloading::Symbol<OodleLZ_DecompressFn> =
            unsafe { lib.get(b"OodleLZ_Decompress")? };
        // Safety: The Symbol lives as long as the Library, which is stored in self.
        let decompress = unsafe {
            std::mem::transmute::<
                libloading::Symbol<'_, OodleLZ_DecompressFn>,
                libloading::Symbol<'static, OodleLZ_DecompressFn>,
            >(decompress)
        };
        Ok(Oodle {
            _lib: lib,
            decompress,
        })
    }

    /// Decompress `src` into `dst` using `scratch` as temporary workspace.
    /// Returns the number of bytes written to `dst`, or 0 on failure.
    pub fn decompress(&self, src: &[u8], dst: &mut [u8], scratch: &[u8]) -> u64 {
        unsafe {
            (self.decompress)(
                src.as_ptr(),
                src.len() as u64,
                dst.as_mut_ptr(),
                dst.len() as u64,
                1,  // fuzz_safe
                0,  // check_crc
                3,  // verbose
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                scratch.len() as u64,
                3,  // demand_continue
            )
        }
    }
}
