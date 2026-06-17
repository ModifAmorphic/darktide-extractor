//! Discovery utilities for game directories, bundle directories, and Oodle libraries.

use std::path::{Path, PathBuf};

use anyhow::Result;
use darktide_bundle::Oodle;

/// Sentinel file to validate game directories.
const GAME_DIR_SENTINEL: &str = "bundle/bundle_database.data";

/// Default Oodle library name (platform-specific).
#[cfg(target_os = "windows")]
const DEFAULT_OODLE_LIB: &str = "oo2core_9_win64.dll";

#[cfg(not(target_os = "windows"))]
const DEFAULT_OODLE_LIB: &str = "liboo2corelinux64.so.9";

/// Classification of user input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputKind {
    /// A single bundle file.
    SingleBundle(PathBuf),
    /// A directory containing bundles (the `bundle/` directory itself).
    BundleDir(PathBuf),
}

/// Resolve the game directory from explicit, env, or Steam discovery.
pub fn resolve_game_dir(explicit: Option<&Path>) -> Option<PathBuf> {
    // 1. Explicit path
    if let Some(path) = explicit {
        if validate_game_dir(path) {
            return Some(path.to_path_buf());
        }
        return None;
    }

    // 2. Environment variable
    if let Ok(env_path) = std::env::var("DARKTIDE_DIR") {
        let path = PathBuf::from(env_path);
        if validate_game_dir(&path) {
            return Some(path);
        }
    }

    // 3. Steam auto-discovery
    discover_steam_game()
}

/// Validate that a path is a valid Darktide game directory.
fn validate_game_dir(path: &Path) -> bool {
    path.join(GAME_DIR_SENTINEL).exists()
}

/// Discover the Darktide game installation via Steam (best-effort).
pub fn discover_steam_game() -> Option<PathBuf> {
    let steam_root = discover_steam_root()?;
    let library_paths = discover_library_paths(&steam_root);

    for lib in library_paths {
        let game_path = lib.join("steamapps/common/Warhammer 40,000 DARKTIDE");
        if game_path.exists() && validate_game_dir(&game_path) {
            return Some(game_path);
        }
    }

    None
}

/// Discover the Steam installation directory.
fn discover_steam_root() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        use winreg::enums::HKEY_CURRENT_USER;
        use winreg::RegKey;

        let hklm = RegKey::predef(HKEY_CURRENT_USER);
        let subkey = hklm.open_subkey("Software\\Valve\\Steam").ok()?;
        let steam_path: String = subkey.get_value("SteamPath").ok()?;
        Some(PathBuf::from(steam_path))
    }

    #[cfg(not(target_os = "windows"))]
    {
        let home = std::env::var("HOME").ok()?;
        let candidates = [
            format!("{}/.steam/steam", home),
            format!("{}/.local/share/Steam", home),
            format!("{}/.var/app/com.valvesoftware.Steam/data/Steam", home),
        ];

        for candidate in candidates {
            let path = PathBuf::from(&candidate);
            if path.exists() {
                return Some(path);
            }
        }
        None
    }
}

/// Discover all Steam library folders from libraryfolders.vdf.
fn discover_library_paths(steam_root: &Path) -> Vec<PathBuf> {
    let mut libraries = vec![steam_root.to_path_buf()];

    let vdf_path = steam_root.join("steamapps/libraryfolders.vdf");
    if !vdf_path.exists() {
        return libraries;
    }

    // Read VDF content
    let vdf_text = match std::fs::read_to_string(&vdf_path) {
        Ok(text) => text,
        Err(_) => return libraries,
    };

    // Extract "path" values
    for path_str in vdf_library_paths(&vdf_text) {
        libraries.push(PathBuf::from(path_str));
    }

    libraries
}

/// Extract "path" values from a libraryfolders.vdf string.
/// This is a simple hand-rolled scanner, not a full VDF parser.
pub fn vdf_library_paths(vdf_text: &str) -> Vec<String> {
    let mut paths = Vec::new();
    let mut chars = vdf_text.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '"' {
            // Try to match "path"
            let key: String = chars.by_ref().take_while(|&ch| ch != '"').collect();
            if key == "path" {
                // Skip whitespace and opening quote
                while chars
                    .peek()
                    .is_some_and(|&ch| ch.is_whitespace() || ch == '"')
                {
                    chars.next();
                }
                // Read path value
                let value: String = chars.by_ref().take_while(|&ch| ch != '"').collect();
                if !value.is_empty() {
                    paths.push(value);
                }
            } else {
                // Skip the rest of this quoted value
                while chars.peek().is_some_and(|&ch| ch != '"') {
                    chars.next();
                }
            }
        }
    }

    paths
}

/// Resolve user input to either a single bundle or a bundle directory.
pub fn resolve_input(input: Option<&Path>, game_dir: Option<&Path>) -> Result<InputKind> {
    // 1. If input is given
    if let Some(path) = input {
        if !path.exists() {
            anyhow::bail!("Input path does not exist: {}", path.display());
        }

        if path.is_file() {
            // Single bundle file
            return Ok(InputKind::SingleBundle(path.to_path_buf()));
        }

        if path.is_dir() {
            // Check if it's a bundle dir (contains bundle_database.data)
            if path.join("bundle_database.data").exists() {
                return Ok(InputKind::BundleDir(path.to_path_buf()));
            }

            // Check if it's a game root (contains bundle/bundle_database.data)
            let bundle_dir = path.join("bundle");
            if bundle_dir.join("bundle_database.data").exists() {
                return Ok(InputKind::BundleDir(bundle_dir));
            }

            anyhow::bail!(
                "Input directory is not a bundle directory or game root: {}",
                path.display()
            );
        }

        anyhow::bail!(
            "Input path is neither a file nor a directory: {}",
            path.display()
        );
    }

    // 2. If game_dir is Some
    if let Some(game_root) = game_dir {
        let bundle_dir = game_root.join("bundle");
        if !bundle_dir.is_dir() {
            anyhow::bail!(
                "Game directory does not contain a bundle directory: {}",
                game_root.display()
            );
        }
        return Ok(InputKind::BundleDir(bundle_dir));
    }

    // 3. Nothing provided
    Err(anyhow::anyhow!(
        "Couldn't find the Darktide game automatically. Pass `-i <bundle dir>` (or a game root / single bundle), or `--game-dir <game dir>`. See `dtex --help`."
    ))
}

/// Resolve the Oodle library from explicit, env, game-dir, or discovery.
pub fn resolve_oodle(
    oodle_lib: Option<&str>,
    game_dir: Option<&Path>,
    verbose: bool,
) -> Result<Oodle> {
    let mut errors = Vec::new();

    // 1. Explicit path
    if let Some(path) = oodle_lib {
        return Oodle::load_verbose(path, verbose).map_err(|e| {
            anyhow::anyhow!("Failed to load Oodle from explicit path '{}': {}", path, e)
        });
    }

    // 2. Environment variable
    if let Ok(env_path) = std::env::var("DTEX_OODLE_LIB") {
        match Oodle::load_verbose(&env_path, verbose) {
            Ok(oodle) => return Ok(oodle),
            Err(e) => {
                errors.push(format!("env DTEX_OODLE_LIB={}: {}", env_path, e));
            }
        }
    }

    // 3. Windows: game-dir/binaries/oo2core_9_win64.dll
    #[cfg(target_os = "windows")]
    {
        if let Some(game_root) = game_dir {
            let dll_path = game_root.join("binaries/oo2core_9_win64.dll");
            match dll_path.to_str() {
                Some(path_str) => match Oodle::load_verbose(path_str, verbose) {
                    Ok(oodle) => return Ok(oodle),
                    Err(e) => {
                        errors.push(format!("game-dir binaries: {}", e));
                    }
                },
                None => {
                    errors.push(format!(
                        "game-dir binaries: path is not valid UTF-8: {}",
                        dll_path.display()
                    ));
                }
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = game_dir; // Suppress unused warning on non-Windows
    }

    // 4. Discovery
    match Oodle::discover_verbose(DEFAULT_OODLE_LIB, verbose) {
        Ok(oodle) => return Ok(oodle),
        Err(e) => {
            errors.push(format!("discovery: {}", e));
        }
    }

    // All attempts failed
    let mut msg = "Failed to load Oodle library. Tried:\n".to_string();
    for err in &errors {
        msg.push_str("  ");
        msg.push_str(err);
        msg.push('\n');
    }

    #[cfg(target_os = "windows")]
    {
        msg.push('\n');
        msg.push_str("On Windows, the DLL ships with the game in <game-dir>/binaries/. ");
        msg.push_str("Try using --game-dir <path> if Steam auto-discovery fails.\n");
    }

    #[cfg(not(target_os = "windows"))]
    {
        msg.push('\n');
        msg.push_str("On Linux, see docs/oodle-library.md for how to obtain the library.\n");
    }

    msg.push_str("See docs/oodle-library.md for more information.");

    Err(anyhow::anyhow!(msg))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vdf_library_paths_simple() {
        let vdf = r#"
            "libraryfolders"
            {
                "0"
                {
                    "path"		"/home/user/.steam/steam"
                    "label"		""
                    "contentid"		"228980"
                    "totalsize"		"0"
                    "update_clean_bytes"		"0"
                    "time_last_update_corruption"		"0"
                    "apps"
                    {
                    }
                }
                "1"
                {
                    "path"		"/mnt/games"
                    "label"		"Games"
                    "contentid"		"228980"
                    "totalsize"		"0"
                    "update_clean_bytes"		"0"
                    "time_last_update_corruption"		"0"
                    "apps"
                    {
                    }
                }
            }
        "#;

        let paths = vdf_library_paths(vdf);
        assert_eq!(paths.len(), 2);
        assert!(paths.contains(&"/home/user/.steam/steam".to_string()));
        assert!(paths.contains(&"/mnt/games".to_string()));
    }

    #[test]
    fn test_vdf_library_paths_empty() {
        let vdf = r#"{}"#;
        let paths = vdf_library_paths(vdf);
        assert_eq!(paths.len(), 0);
    }

    #[test]
    fn test_vdf_library_paths_no_path() {
        let vdf = r#"
            "libraryfolders"
            {
                "0"
                {
                    "label"		"Main Library"
                    "contentid"		"228980"
                }
            }
        "#;
        let paths = vdf_library_paths(vdf);
        assert_eq!(paths.len(), 0);
    }

    #[test]
    fn test_resolve_input_single_bundle() {
        // This test requires a real file, so we'll use a temp file
        let temp_file = std::env::temp_dir().join("test.bundle");
        std::fs::write(&temp_file, b"test").unwrap();

        let result = resolve_input(Some(&temp_file), None);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), InputKind::SingleBundle(temp_file.clone()));

        std::fs::remove_file(&temp_file).unwrap();
    }

    #[test]
    fn test_resolve_input_bundle_dir() {
        let temp_dir = std::env::temp_dir().join("test_bundle_dir");
        std::fs::create_dir_all(&temp_dir).unwrap();
        std::fs::write(temp_dir.join("bundle_database.data"), b"test").unwrap();

        let result = resolve_input(Some(&temp_dir), None);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), InputKind::BundleDir(temp_dir.clone()));

        std::fs::remove_dir_all(&temp_dir).unwrap();
    }

    #[test]
    fn test_resolve_input_game_root() {
        let temp_dir = std::env::temp_dir().join("test_game_root");
        let bundle_dir = temp_dir.join("bundle");
        std::fs::create_dir_all(&bundle_dir).unwrap();
        std::fs::write(bundle_dir.join("bundle_database.data"), b"test").unwrap();

        let result = resolve_input(Some(&temp_dir), None);
        assert!(result.is_ok());
        // Should return the bundle directory, not the game root
        assert_eq!(result.unwrap(), InputKind::BundleDir(bundle_dir));

        std::fs::remove_dir_all(&temp_dir).unwrap();
    }

    #[test]
    fn test_resolve_input_invalid_dir() {
        let temp_dir = std::env::temp_dir().join("test_invalid_dir");
        std::fs::create_dir_all(&temp_dir).unwrap();

        let result = resolve_input(Some(&temp_dir), None);
        assert!(result.is_err());

        std::fs::remove_dir_all(&temp_dir).unwrap();
    }

    #[test]
    fn test_resolve_input_nonexistent() {
        let nonexistent = std::env::temp_dir().join("does_not_exist");
        let result = resolve_input(Some(&nonexistent), None);
        assert!(result.is_err());
    }

    #[test]
    fn test_resolve_input_with_game_dir() {
        // Create a temp game dir
        let game_root = std::env::temp_dir().join("test_game_root_2");
        let bundle_dir = game_root.join("bundle");
        std::fs::create_dir_all(&bundle_dir).unwrap();
        std::fs::write(bundle_dir.join("bundle_database.data"), b"test").unwrap();

        // When input is None, should use game_dir
        let result = resolve_input(None, Some(&game_root));
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), InputKind::BundleDir(bundle_dir));

        std::fs::remove_dir_all(&game_root).unwrap();
    }

    #[test]
    fn test_resolve_input_nothing_provided() {
        let result = resolve_input(None, None);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Couldn't find the Darktide game"));
    }

    #[test]
    fn test_resolve_game_dir_explicit_valid() {
        let temp_dir = std::env::temp_dir().join("test_game_root_explicit_valid");
        let bundle_dir = temp_dir.join("bundle");
        std::fs::create_dir_all(&bundle_dir).unwrap();
        std::fs::write(bundle_dir.join("bundle_database.data"), b"test").unwrap();

        let result = resolve_game_dir(Some(&temp_dir));
        assert_eq!(result, Some(temp_dir.clone()));

        std::fs::remove_dir_all(&temp_dir).unwrap();
    }

    #[test]
    fn test_resolve_game_dir_explicit_invalid() {
        let temp_dir = std::env::temp_dir().join("test_game_root_explicit_invalid");
        std::fs::create_dir_all(&temp_dir).unwrap();

        let result = resolve_game_dir(Some(&temp_dir));
        assert_eq!(result, None);

        std::fs::remove_dir_all(&temp_dir).unwrap();
    }

    #[test]
    fn test_validate_game_dir_valid() {
        let temp_dir = std::env::temp_dir().join("test_valid_game");
        let bundle_dir = temp_dir.join("bundle");
        std::fs::create_dir_all(&bundle_dir).unwrap();
        std::fs::write(bundle_dir.join("bundle_database.data"), b"test").unwrap();

        assert!(validate_game_dir(&temp_dir));

        std::fs::remove_dir_all(&temp_dir).unwrap();
    }

    #[test]
    fn test_validate_game_dir_invalid() {
        let temp_dir = std::env::temp_dir().join("test_invalid_game");
        std::fs::create_dir_all(&temp_dir).unwrap();

        assert!(!validate_game_dir(&temp_dir));

        std::fs::remove_dir_all(&temp_dir).unwrap();
    }
}
