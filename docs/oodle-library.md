# Oodle Library

Darktide bundles use Oodle (Kraken) compression. The Oodle shared library is a proprietary
Epic Games component required at runtime; it is not open source. `dtex` loads it dynamically
via `libloading` (`dlopen` on Linux, `LoadLibrary` on Windows).

Neither library is redistributed in this repository (both are listed in `.gitignore`). CI workflows download them automatically before running tests; developers obtain them locally as described below.

## Auto-discovery

`dtex` searches for the Oodle library in the following order:

1. `--oodle-lib <path>` command-line flag
2. `DTEX_OODLE_LIB` environment variable
3. Windows only: `<game-dir>/binaries/oo2core_9_win64.dll` (if `--game-dir` is provided or auto-discovered)
4. Next to the `dtex` binary (exe-dir)
5. Current working directory
6. System library search path (bare library name)

If the library cannot be found, `dtex` prints an error with platform-specific hints:
- On Linux, the error message references this document.
- On Windows, the error message mentions that the DLL ships with the game in `<game-dir>/binaries/` and suggests using `--game-dir` if Steam auto-discovery fails.

## Supported platforms

| Platform | Library file | Oodle version | Size |
|---|---|---|---|
| Linux | `liboo2corelinux64.so.9` | 2.9.14 | 688,096 bytes |
| Windows | `oo2core_9_win64.dll` | 2.9.10 | 637,952 bytes |

The Oodle FFI signature (`OodleLZ_Decompress`) is identical across platforms. The Linux
build uses a 14-argument calling convention where the scratch argument is a size, not a
pointer. The signature is reverse-engineered and verified empirically against 9,648 Darktide
bundles (see [`crates/darktide-bundle/src/oodle.rs`](../crates/darktide-bundle/src/oodle.rs)).

## Obtaining the library

The library is distributed as an Unreal Engine build dependency on Epic's CDN. The process
requires a `Commit.gitdeps.xml` file from the
[EpicGames/UnrealEngine](https://github.com/EpicGames/UnrealEngine) repo (requires GitHub
org membership; free signup at https://github.com/EpicGames/Signup).

The XML links four elements to locate a file:

1. **`<DependencyManifest>`** has `BaseUrl` (e.g. `https://cdn.unrealengine.com/dependencies`)
2. **`<File>`** has `Name` and `Hash` — find the file by name
3. **`<Blob>`** matches `Hash` to a `File`; provides `Size`, `PackOffset`, and `PackHash`
4. **`<Pack>`** matches `Hash` to a `Blob`'s `PackHash`; provides `RemotePath`

Download URL: `{BaseUrl}/{Pack.RemotePath}/{Pack.Hash}` — the response is gzip-compressed.
Seek to `Blob.PackOffset`, then read `Blob.Size` bytes.

### Linux (v2.9.14)

XML trace:

```xml
<File Name="Engine/Source/Runtime/OodleDataCompression/Sdks/2.9.14/lib/Linux/liboo2corelinux64.so.9"
      Hash="ff1f6d0faa4fceaeec9d4c1a0a391160dfe78b54" />

<Blob Hash="ff1f6d0faa4fceaeec9d4c1a0a391160dfe78b54"
      Size="688096"
      PackHash="4f6c5fd233cb85f91497bd8c722fd7a89f1c657a"
      PackOffset="1399275" />

<Pack Hash="4f6c5fd233cb85f91497bd8c722fd7a89f1c657a"
      RemotePath="UnrealEngine-42566482" />
```

Download (Linux/macOS):

```sh
curl -sL "https://cdn.unrealengine.com/dependencies/UnrealEngine-42566482/4f6c5fd233cb85f91497bd8c722fd7a89f1c657a" \
  | gunzip \
  | dd bs=1 skip=1399275 count=688096 of=liboo2corelinux64.so.9
```

Result: 672 KB ELF shared object, MD5 `18aa46f51f41f8c81cde1636ad486c81`.

### Windows (v2.9.10)

XML trace:

- Pack RemotePath: `UnrealEngine-27563807`
- Pack Hash: `51bf6515dd35ac8361c9a324b6deb1736a61240c`
- PackOffset: `1240856`
- Size: `637952`

Download (Linux/macOS):

```sh
curl -sL "https://cdn.unrealengine.com/dependencies/UnrealEngine-27563807/51bf6515dd35ac8361c9a324b6deb1736a61240c" \
  | gunzip \
  | dd bs=1 skip=1240856 count=637952 of=oo2core_9_win64.dll
```

Download (Windows PowerShell):

```powershell
$url = "https://cdn.unrealengine.com/dependencies/UnrealEngine-27563807/51bf6515dd35ac8361c9a324b6deb1736a61240c"
$response = Invoke-WebRequest -Uri $url -UseBasicParsing
$gzip = [System.IO.Compression.GzipStream]::new(
    [System.IO.MemoryStream]::new($response.Content),
    [System.IO.Compression.CompressionMode]::Decompress
)
$bytes = [System.IO.MemoryStream]::new()
$gzip.CopyTo($bytes)
$gzip.Dispose()
$data = $bytes.ToArray()
$offset = 1240856
$count = 637952
$dll = $data[$offset..($offset + $count - 1)]
[System.IO.File]::WriteAllBytes("oo2core_9_win64.dll", $dll)
```

Result: 623 KB PE DLL, Oodle 2.9.10.
