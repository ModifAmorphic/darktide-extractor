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

## README.md

- Maintain a `README.md` that summarizes the purpose of this repository.
- Include instructions for using CLI arguments (assume downloadable binaries via CI/Release workflows).
- Update the README whenever relevant changes are made (new features, CLI argument changes, etc.).
- **Markdown rules for human-facing docs:**
  - No directory tree diagrams of every file. Cover only core directories, not deep subdirectory structures.
  - No emojis or unicode icons.
