#pragma once

// C ABI for Darktide bundle extraction.
// Mirrors crates/darktide-ffi/src/lib.rs — that source file is the source of truth.

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

// ---------------------------------------------------------------------------
// Error convention
// ---------------------------------------------------------------------------
// - Functions returning pointers return non-NULL on success, NULL on failure.
// - Functions returning int return 0 on success, -1 on failure.
// - darktide_bundle_file_data returns byte count >= 0 on success, -1 on failure.
// - darktide_lookup_extension returns a program-lifetime string pointer or NULL.
//
// Memory management:
// - Every handle returned by a *_load/*_open/*_read_*/*_extract_* function
//   MUST be freed by its paired *_free function, exactly once.
// - Double-free is undefined behavior.

// ---------------------------------------------------------------------------
// Opaque handle types
// ---------------------------------------------------------------------------

typedef struct DarktideOodle DarktideOodle;
typedef struct DarktideBundle DarktideBundle;
typedef struct DarktideIndex DarktideIndex;
typedef struct DarktideFiles DarktideFiles;

// ---------------------------------------------------------------------------
// C-compatible structs
// ---------------------------------------------------------------------------

typedef struct {
    uint64_t ext;
    uint64_t name;
    uint32_t mode;
} DarktideIndexEntry;

typedef struct {
    uint64_t ext;
    uint64_t name;
    uint32_t num_variants;
    uint64_t data_len;
} DarktideFileEntry;

// ---------------------------------------------------------------------------
// Oodle
// ---------------------------------------------------------------------------

DarktideOodle* darktide_oodle_load(const char* path);
void darktide_oodle_free(DarktideOodle* oodle);

// ---------------------------------------------------------------------------
// Bundle
// ---------------------------------------------------------------------------

DarktideBundle* darktide_bundle_open(const char* path);
void darktide_bundle_free(DarktideBundle* bundle);

// ---------------------------------------------------------------------------
// Index
// ---------------------------------------------------------------------------

DarktideIndex* darktide_bundle_read_index(DarktideBundle* bundle);
void darktide_bundle_index_free(DarktideIndex* index);
uint32_t darktide_bundle_index_count(const DarktideIndex* index);
int darktide_bundle_index_entry(const DarktideIndex* index, uint32_t idx, DarktideIndexEntry* out);

// ---------------------------------------------------------------------------
// Extraction
// ---------------------------------------------------------------------------

DarktideFiles* darktide_bundle_extract_all(DarktideBundle* bundle, DarktideOodle* oodle);
void darktide_bundle_files_free(DarktideFiles* files);
uint32_t darktide_bundle_files_count(const DarktideFiles* files);
int darktide_bundle_file_entry(const DarktideFiles* files, uint32_t idx, DarktideFileEntry* out);
int64_t darktide_bundle_file_data(const DarktideFiles* files, uint32_t idx, uint8_t* out_buf, uint64_t out_len);

// ---------------------------------------------------------------------------
// Hash utilities
// ---------------------------------------------------------------------------

uint64_t darktide_murmur_hash64(const uint8_t* data, uint32_t len);
const char* darktide_lookup_extension(uint64_t hash);

#ifdef __cplusplus
}
#endif