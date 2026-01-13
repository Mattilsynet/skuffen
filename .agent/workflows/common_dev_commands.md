---
description: Run common development commands automatically
---

This workflow defines safe read-only or standard build/git commands that can be run without manual confirmation.

// turbo

1. Check the code
   `cargo check`

// turbo
2. Run tests
   `cargo test`

// turbo
3. Build the project
   `cargo build`

// turbo
4. Format check
   `cargo fmt -- --check`

// turbo
5. Clippy check
   `cargo clippy`

// turbo
6. Check git status
   `git status`

// turbo
7. Stage all changes
   `git add .`
