//! Integration tests for CLI commands using synthetic bundles.
//!
//! These tests use the real Oodle library at the repo root to ensure
//! the full extraction pipeline works end-to-end.

use darktide_bundle::{testutil::SyntheticEntry, Oodle};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::str;

/// Get the Oodle library path for the current platform.
/// Asserts that the library exists and loads correctly.
fn oodle_path() -> PathBuf {
    // Use CARGO_MANIFEST_DIR which is the repo root for integration tests in tests/
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    let lib_name = if cfg!(target_os = "windows") {
        "oo2core_9_win64.dll"
    } else {
        "liboo2corelinux64.so.9"
    };

    let lib_path = manifest.join(lib_name);

    assert!(
        lib_path.exists(),
        "Oodle library not found at {}. See docs/oodle-library.md for setup instructions.",
        lib_path.display()
    );

    lib_path
}

/// Create a temporary directory for testing.
fn temp_dir() -> tempfile::TempDir {
    tempfile::tempdir().expect("Failed to create temp dir")
}

/// Write a synthetic bundle with the given entries.
fn write_synthetic_bundle(path: &Path, entries: &[SyntheticEntry]) {
    let lib_path = oodle_path();
    let oodle = Oodle::load(lib_path.to_str().unwrap()).expect("Failed to load Oodle");
    darktide_bundle::testutil::write_synthetic_bundle(path, entries, &oodle)
        .expect("Failed to write synthetic bundle");
}

/// Run dtex as a subprocess and capture stdout/stderr.
fn run_dtex(args: &[&str]) -> (String, String, i32) {
    let bin =
        std::env::var("CARGO_BIN_EXE_DTEX").unwrap_or_else(|_| "target/debug/dtex".to_string());
    let mut cmd = Command::new(&bin);
    cmd.args(args);

    let output = cmd.output().expect("Failed to run dtex");
    let stdout = str::from_utf8(&output.stdout).unwrap_or("").to_string();
    let stderr = str::from_utf8(&output.stderr).unwrap_or("").to_string();
    let exit_code = output.status.code().unwrap_or(-1);

    (stdout, stderr, exit_code)
}

/// Run dtex with JSON output and parse as JSON.
fn run_dtex_json(args: &[&str]) -> serde_json::Value {
    let (stdout, stderr, exit_code) = run_dtex(args);
    assert_eq!(
        exit_code, 0,
        "dtex exited with {}: stderr: {}",
        exit_code, stderr
    );
    serde_json::from_str(&stdout).expect("Failed to parse JSON output")
}

#[test]
fn dir_extract_skips_nonbundles() {
    let lib_path = oodle_path();
    let temp = temp_dir();
    let bundle_dir = temp.path().join("bundles");
    fs::create_dir_all(&bundle_dir).unwrap();

    // Create two synthetic bundles with different name hashes
    let bundle1 = bundle_dir.join("bundle1.bundle");
    write_synthetic_bundle(
        &bundle1,
        &[SyntheticEntry {
            ext: "lua".to_string(),
            name: "script1".to_string(),
            body: b"-- script1".to_vec(),
        }],
    );

    let bundle2 = bundle_dir.join("bundle2.bundle");
    write_synthetic_bundle(
        &bundle2,
        &[SyntheticEntry {
            ext: "lua".to_string(),
            name: "script2".to_string(),
            body: b"-- script2".to_vec(),
        }],
    );

    // Create a non-bundle file (make sure it's at least 8 bytes to be readable)
    let non_bundle = bundle_dir.join("not_a_bundle.txt");
    fs::write(&non_bundle, b"this is not a bundle").unwrap();

    // Create bundle_database.data so the directory is recognized as a bundle dir
    // Must be at least 8 bytes for Bundle::classify to read the magic bytes
    fs::write(bundle_dir.join("bundle_database.data"), b"not_a_bundle_db").unwrap();

    let output_dir = temp.path().join("output");

    // Extract with Oodle lib specified
    let (stdout, stderr, exit_code) = run_dtex(&[
        "--oodle-lib",
        lib_path.to_str().unwrap(),
        "extract",
        "-i",
        bundle_dir.to_str().unwrap(),
        "-o",
        output_dir.to_str().unwrap(),
        "--json",
    ]);

    assert_eq!(exit_code, 0, "dtex exited with {}: {}", exit_code, stderr);

    let result: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    eprintln!(
        "JSON result: {}",
        serde_json::to_string_pretty(&result).unwrap()
    );
    assert_eq!(result["bundles_processed"], 2);
    assert_eq!(result["bundles_skipped_nonbundle"], 2); // not_a_bundle.txt + bundle_database.data
    assert_eq!(result["files_extracted"], 2);
    assert_eq!(result["errors"], 0);

    // Check that exactly 2 files were extracted under lua/ (don't assert exact hashes)
    let lua_dir = output_dir.join("lua");
    assert!(lua_dir.exists(), "lua/ directory should exist");
    let lua_files: Vec<_> = fs::read_dir(&lua_dir)
        .expect("Failed to read lua/ directory")
        .collect();
    assert_eq!(lua_files.len(), 2, "Should have exactly 2 files in lua/");
}

#[test]
fn dir_extract_collisions() {
    let lib_path = oodle_path();

    let temp = temp_dir();
    let bundle_dir = temp.path().join("bundles");
    fs::create_dir_all(&bundle_dir).unwrap();

    // Create bundle_database.data so the directory is recognized as a bundle dir
    // Must be at least 8 bytes for Bundle::classify to read the magic bytes
    fs::write(bundle_dir.join("bundle_database.data"), b"not_a_bundle_db").unwrap();

    let output_dir = temp.path().join("output");

    // Create two bundles that will produce the same output path
    let bundle1 = bundle_dir.join("bundle1.bundle");
    write_synthetic_bundle(
        &bundle1,
        &[SyntheticEntry {
            ext: "lua".to_string(),
            name: "same_name".to_string(),
            body: b"-- version 1".to_vec(),
        }],
    );

    let bundle2 = bundle_dir.join("bundle2.bundle");
    write_synthetic_bundle(
        &bundle2,
        &[SyntheticEntry {
            ext: "lua".to_string(),
            name: "same_name".to_string(), // Same name hash
            body: b"-- version 2".to_vec(),
        }],
    );

    // Test default overwrite mode
    let (stdout, stderr, exit_code) = run_dtex(&[
        "--oodle-lib",
        lib_path.to_str().unwrap(),
        "extract",
        "-i",
        bundle_dir.to_str().unwrap(),
        "-o",
        output_dir.to_str().unwrap(),
        "--on-collision",
        "overwrite",
        "--json",
    ]);

    assert_eq!(exit_code, 0, "dtex exited with {}: {}", exit_code, stderr);

    let result: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(result["files_extracted"], 2); // Both bundles extracted
    assert_eq!(result["collisions"], 1);

    // Check that only one file exists (last write wins)
    // Hash of "same_name" is 0x3b7fddd0b1db8626
    let lua_file = output_dir.join("lua").join("3b7fddd0b1db8626");
    assert!(lua_file.exists());

    // Check that stderr shows collision report (FIX 3: now prints to stderr even with --json)
    assert!(stderr.contains("Collisions:"));

    // Clean up for next test
    fs::remove_dir_all(&output_dir).unwrap();

    // Test skip mode
    let (stdout, stderr, exit_code) = run_dtex(&[
        "--oodle-lib",
        lib_path.to_str().unwrap(),
        "extract",
        "-i",
        bundle_dir.to_str().unwrap(),
        "-o",
        output_dir.to_str().unwrap(),
        "--on-collision",
        "skip",
        "--json",
    ]);

    assert_eq!(exit_code, 0, "dtex exited with {}: {}", exit_code, stderr);

    let result: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(result["files_extracted"], 1); // Only first file written (FIX 2)
    assert_eq!(result["collisions"], 1);

    // Check that the file exists and is from one of the bundles
    // Hash of "same_name" is 0x3b7fddd0b1db8626
    let lua_file = output_dir.join("lua").join("3b7fddd0b1db8626");
    assert!(lua_file.exists());
    let content = fs::read_to_string(&lua_file).unwrap();
    assert!(
        content.contains("version 1") || content.contains("version 2"),
        "content should contain one of the two valid versions, not be corrupted/empty"
    );

    // Check that stderr shows collision report
    assert!(stderr.contains("Collisions:"));

    // Clean up for next test
    fs::remove_dir_all(&output_dir).unwrap();

    // Test error mode
    let (_stdout, stderr, exit_code) = run_dtex(&[
        "--oodle-lib",
        lib_path.to_str().unwrap(),
        "extract",
        "-i",
        bundle_dir.to_str().unwrap(),
        "-o",
        output_dir.to_str().unwrap(),
        "--on-collision",
        "error",
        "--json",
    ]);

    assert_ne!(
        exit_code, 0,
        "dtex should exit with non-zero on error mode collision"
    );

    // The JSON output may still be printed before the error, but check for error in stderr
    assert!(stderr.contains("Collision:") || stderr.contains("error"));

    // Clean up for next test
    let _ = fs::remove_dir_all(&output_dir);
}

#[test]
fn find_with_extension() {
    let _lib_path = oodle_path();

    let temp = temp_dir();
    let bundle_dir = temp.path().join("bundles");
    fs::create_dir_all(&bundle_dir).unwrap();

    // Create bundle_database.data so the directory is recognized as a bundle dir
    // Must be at least 8 bytes for Bundle::classify to read the magic bytes
    fs::write(bundle_dir.join("bundle_database.data"), b"not_a_bundle_db").unwrap();

    // Create two bundles with different extensions
    let bundle1 = bundle_dir.join("bundle1.bundle");
    write_synthetic_bundle(
        &bundle1,
        &[
            SyntheticEntry {
                ext: "lua".to_string(),
                name: "script1".to_string(),
                body: b"-- script1".to_vec(),
            },
            SyntheticEntry {
                ext: "texture".to_string(),
                name: "tex1".to_string(),
                body: vec![0u8; 100],
            },
        ],
    );
    eprintln!("Created bundle1: {:?}", bundle1);

    let bundle2 = bundle_dir.join("bundle2.bundle");
    write_synthetic_bundle(
        &bundle2,
        &[SyntheticEntry {
            ext: "lua".to_string(),
            name: "script2".to_string(),
            body: b"-- script2".to_vec(),
        }],
    );
    eprintln!("Created bundle2: {:?}", bundle2);

    // Test TSV output
    let (stdout, stderr, exit_code) =
        run_dtex(&["find", "lua", "-i", bundle_dir.to_str().unwrap()]);

    assert_eq!(exit_code, 0, "dtex exited with {}: {}", exit_code, stderr);

    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 2); // Two lua entries

    // Test JSON output
    let (stdout, stderr, exit_code) =
        run_dtex(&["find", "lua", "-i", bundle_dir.to_str().unwrap(), "--json"]);
    eprintln!("stdout: {}", stdout);
    eprintln!("stderr: {}", stderr);
    eprintln!("exit_code: {}", exit_code);
    let result = run_dtex_json(&["find", "lua", "-i", bundle_dir.to_str().unwrap(), "--json"]);

    assert!(result.is_array());
    let entries = result.as_array().unwrap();
    assert_eq!(entries.len(), 2);

    // Check that all entries have the expected structure
    for entry in entries {
        assert!(entry["bundle"].is_string());
        assert!(entry["name_hash"].is_string());
        assert_eq!(entry["ext"], "lua");
        assert!(entry["mode"].is_number());
    }
}

#[test]
fn find_summary_no_extension() {
    let _lib_path = oodle_path();

    let temp = temp_dir();
    let bundle_dir = temp.path().join("bundles");
    fs::create_dir_all(&bundle_dir).unwrap();

    // Create bundle_database.data so the directory is recognized as a bundle dir
    // Must be at least 8 bytes for Bundle::classify to read the magic bytes
    fs::write(bundle_dir.join("bundle_database.data"), b"not_a_bundle_db").unwrap();

    // Create two bundles
    let bundle1 = bundle_dir.join("bundle1.bundle");
    write_synthetic_bundle(
        &bundle1,
        &[
            SyntheticEntry {
                ext: "lua".to_string(),
                name: "script1".to_string(),
                body: b"-- script1".to_vec(),
            },
            SyntheticEntry {
                ext: "texture".to_string(),
                name: "tex1".to_string(),
                body: vec![0u8; 100],
            },
        ],
    );

    let bundle2 = bundle_dir.join("bundle2.bundle");
    write_synthetic_bundle(
        &bundle2,
        &[SyntheticEntry {
            ext: "lua".to_string(),
            name: "script2".to_string(),
            body: b"-- script2".to_vec(),
        }],
    );

    // Test TSV output
    let (stdout, stderr, exit_code) = run_dtex(&["find", "-i", bundle_dir.to_str().unwrap()]);

    assert_eq!(exit_code, 0, "dtex exited with {}: {}", exit_code, stderr);

    let lines: Vec<&str> = stdout.lines().collect();
    assert!(lines.len() >= 2); // At least lua and texture

    // Test JSON output
    let result = run_dtex_json(&["find", "-i", bundle_dir.to_str().unwrap(), "--json"]);

    assert!(result.is_array());
    let entries = result.as_array().unwrap();

    // Check that lua has count 2 and texture has count 1
    let lua_entry = entries
        .iter()
        .find(|e| e["ext"] == "lua")
        .expect("lua entry not found");
    assert_eq!(lua_entry["count"], 2);

    let texture_entry = entries
        .iter()
        .find(|e| e["ext"] == "texture")
        .expect("texture entry not found");
    assert_eq!(texture_entry["count"], 1);
}

#[test]
fn validate_classifies() {
    let _lib_path = oodle_path();

    let temp = temp_dir();
    let bundle_dir = temp.path().join("bundles");
    fs::create_dir_all(&bundle_dir).unwrap();

    // Create bundle_database.data so the directory is recognized as a bundle dir
    // Must be at least 8 bytes for Bundle::classify to read the magic bytes
    fs::write(bundle_dir.join("bundle_database.data"), b"not_a_bundle_db").unwrap();

    // Create one bundle and one non-bundle
    let bundle = bundle_dir.join("bundle1.bundle");
    write_synthetic_bundle(
        &bundle,
        &[SyntheticEntry {
            ext: "lua".to_string(),
            name: "script1".to_string(),
            body: b"-- script1".to_vec(),
        }],
    );

    let non_bundle = bundle_dir.join("not_a_bundle.txt");
    fs::write(&non_bundle, b"this is not a bundle").unwrap();

    // Test JSON output
    let result = run_dtex_json(&["validate", "-i", bundle_dir.to_str().unwrap(), "--json"]);

    eprintln!(
        "JSON result: {}",
        serde_json::to_string_pretty(&result).unwrap()
    );
    assert_eq!(result["bundle"].as_array().unwrap().len(), 1);
    assert_eq!(result["not_bundle"].as_array().unwrap().len(), 2); // not_a_bundle.txt + bundle_database.data
    assert_eq!(result["unreadable"].as_array().unwrap().len(), 0);
    assert_eq!(result["total"], 3);

    // Check samples
    let samples = &result["samples"];
    assert_eq!(samples["bundle"].as_array().unwrap().len(), 1);
    assert_eq!(samples["not_bundle"].as_array().unwrap().len(), 2);
    assert_eq!(samples["unreadable"].as_array().unwrap().len(), 0);
    assert!(samples["bundle"][0]
        .as_str()
        .unwrap()
        .contains("bundle1.bundle"));
    let not_bundle_samples: Vec<_> = samples["not_bundle"].as_array().unwrap().iter().collect();
    assert!(not_bundle_samples
        .iter()
        .any(|s| s.as_str().unwrap().contains("not_a_bundle.txt")));
    assert!(not_bundle_samples
        .iter()
        .any(|s| s.as_str().unwrap().contains("bundle_database.data")));
}
