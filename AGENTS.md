# AGENTS.md

This file provides instructions for AI agents working in this repository.

## Security Rules

- **Always ask for confirmation** before performing write operations (file creation, edits) on paths **outside** this repository.
- **Always ask for confirmation** before executing shell commands that affect paths **outside** this repository.
- Writes and commands targeting paths **inside** this repository are fine without extra confirmation.

## Persistence

- Agents MAY use the `.agents/` directory to persist notes, plans, and context needed across subagent handoffs or session resumes. Keep entries concise and distilled.
- Periodically check on content in `.agents/` for revisions and cleanup. Prompt user before removing files in case they are used in another context.

## Git Rules

- **Never commit directly to `main`.** Always create a feature branch for any change and open a PR.
- **Always ask for confirmation** before performing git write operations: commits, pushes, resets, force pushes, amends, branch creation/deletion, etc. (Branch creation as the first step of a change is expected and does not need a separate confirmation beyond the user's request to do the work.)
- **PRs are squash-merged.** The branch's individual commit history is collapsed into one commit on `main`, so commit granularity on a branch matters less than a clean, well-described PR.
- **PR descriptions follow Conventional Commits format.** When asked to create a PR, write the description as a Conventional Commits-style summary (type + scope as appropriate) with the changes summarized as one bullet per logical line item.
- **Commit Flow:**
  1. Stage changes and draft a commit message following [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/) syntax.
  2. Present the commit message to the user for approval.
  3. On approval, commit. Push only with separate approval.
- **Paging:** Use `--no-pager` with git commands that may page output (e.g., `git log`, `git diff`, `git show`) to avoid blocking on vim/less prompts.

## Markdown and Paths

- In any committed markdown, use paths **relative to the repository root** (e.g., `crates/darktide-bundle/src/bundle.rs`, `docs/bundle-format.md`). Never embed absolute local paths (e.g., `/home/.../darktide-extractor`) — a fresh clone must not reference a specific machine.
- **Markdown rules for human-facing docs:**
  - No directory tree diagrams of every file. Cover only core directories, not deep subdirectory structures.
  - No emojis or unicode icons.
  - README and other user-facing docs describe the current state (the "now"). Changelog docs (CHANGELOG.md) are an exception.

## README.md

- Maintain a `README.md` summarizing the repository's purpose and how to use it.
- Keep an **Installation** section up to date, covering both Linux and Windows, including how to make the `dtex` CLI available on the user's `PATH`.
- Cover **what the CLI (and library) can extract and how**, including a **Limitations** subsection. Update these as limitations are added, removed, or discovered in subsequent changes.
- Move deep technical specifications into `docs/` and link to them from the README rather than inlining long format/protocol details.

## Oodle Library

This project requires `liboo2corelinux64.so.9` (Linux) or `oo2core_9_win64.dll` (Windows) for Oodle decompression at runtime. These are proprietary Epic Games components (Unreal Engine dependencies) and are NOT redistributed in this repository — both are listed in `.gitignore` and exist only on developer machines. CI workflows download them automatically before running tests; developers must obtain them locally using the process in [`docs/oodle-library.md`](docs/oodle-library.md).

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
