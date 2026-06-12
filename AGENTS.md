# AGENTS.md

This file provides instructions for AI agents working in this repository.

## Security Rules

- **Always ask for confirmation** before performing write operations (file creation, edits) on paths **outside** this workspace (`/home/justin/repos/ModifAmorphic/darktide-extractor`).
- **Always ask for confirmation** before executing shell commands that affect paths **outside** this workspace.
- Writes and commands targeting paths **inside** this workspace are fine without extra confirmation.

## Context Persistence

- Create a `CONTEXT.md` file in `.agents/` (create the directory if needed) at the start of each task.
- Continually update `CONTEXT.md` to reflect the current state and progress of each step.
- Keep it **concise and distilled** — include only important details that need to persist (goals, decisions, key findings, current status, next steps). Avoid verbose logs or raw output.
- Target **concise and focused** — avoid bloating the context window when resuming.
- The goal: a new session can read `CONTEXT.md` and continue where we left off.

## Git Rules

- **Always ask for confirmation** before performing git write operations: commits, pushes, resets, force pushes, amends, etc.
- **Commit Flow:**
  1. Stage changes and draft a commit message following [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/) syntax.
  2. Present the commit message to the user for approval.
  3. On approval, commit but **do not push**.
  4. Request separate approval before pushing.
- **Paging:** Use `--no-pager` with git commands that may page output (e.g., `git log`, `git diff`, `git show`) to avoid blocking on vim/less prompts.

## Oodle Library

This project requires `liboo2corelinux64.so.9` (Oodle compression, proprietary). It is vendored in the repo root.

### How it was obtained

The library is distributed as an Unreal Engine build dependency on Epic's CDN. The process requires a `Commit.gitdeps.xml` file from the [EpicGames/UnrealEngine](https://github.com/EpicGames/UnrealEngine) repo (requires GitHub org membership, free signup at https://github.com/EpicGames/Signup).

The XML links four elements to locate a file:

1. **`<DependencyManifest>`** has `BaseUrl` (e.g. `https://cdn.unrealengine.com/dependencies`)
2. **`<File>`** has `Name` and `Hash` — find the file by name
3. **`<Blob>`** matches `Hash` to a `File`, provides `Size`, `PackOffset`, and `PackHash`
4. **`<Pack>`** matches `Hash` to a `Blob`'s `PackHash`, provides `RemotePath`

Download URL: `{BaseUrl}/{Pack.RemotePath}/{Pack.Hash}` — the response is gzip-compressed. Seek to `Blob.PackOffset`, read `Blob.Size` bytes.

### Vendored file details

- **Path**: `liboo2corelinux64.so.9`
- **Version**: Oodle 2.9.14
- **Size**: 688,096 bytes
- **MD5**: `18aa46f51f41f8c81cde1636ad486c81`
- **XML trace**:
  - File Hash: `ff1f6d0faa4fceaeec9d4c1a0a391160dfe78b54`
  - Blob PackHash: `4f6c5fd233cb85f91497bd8c722fd7a89f1c657a`, PackOffset: `1399275`, Size: `688096`
  - Pack RemotePath: `UnrealEngine-42566482`
- **Download command**:
  ```bash
  curl -sL "https://cdn.unrealengine.com/dependencies/UnrealEngine-42566482/4f6c5fd233cb85f91497bd8c722fd7a89f1c657a" \
    | gunzip | dd bs=1 skip=1399275 count=688096 of=liboo2corelinux64.so.9
  ```

## README.md

- Maintain a `README.md` that summarizes the purpose of this repository.
- Include instructions for using CLI arguments (assume downloadable binaries via CI/Release workflows).
- Update the README whenever relevant changes are made (new features, CLI argument changes, etc.).
- **Markdown rules for human-facing docs:**
  - No directory tree diagrams of every file. Cover only core directories, not deep subdirectory structures.
  - No emojis or unicode icons.
