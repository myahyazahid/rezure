# Contributing to Rezure

Thanks for your interest in contributing to Rezure! This document explains how to set up your environment, the coding conventions we follow, and the process for submitting changes.

---

## 📋 Before You Start

- Check existing [Issues](../../issues) and [Pull Requests](../../pulls) to avoid duplicate work.
- For a new feature or significant change, open an issue first to discuss the approach before writing code.
- For small fixes (typos, minor bugs), feel free to open a PR directly.

---

## 🛠️ Development Setup

### Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) (stable toolchain)
- [Node.js](https://nodejs.org/) (LTS) + npm/pnpm
- [Tauri prerequisites for Windows](https://tauri.app/start/prerequisites/)

### Steps

```bash
# Fork the repo, then clone your fork
git clone https://github.com/<your-username>/rezure.git
cd rezure

# Install dependencies
npm install

# Run in development mode
npm run tauri dev
```

See [`docs/setup-dev.md`](docs/setup-dev.md) for a more detailed walkthrough, including troubleshooting common setup issues.

---

## 🌿 Branching Strategy

- `main` — always stable, reflects the latest release
- `develop` — active development branch, PRs target this branch
- Feature branches: `feature/<short-description>` (e.g. `feature/php-version-switcher`)
- Bug fix branches: `fix/<short-description>` (e.g. `fix/port-conflict-detection`)

---

## 💻 Coding Conventions

### Rust (`src-tauri/`)

- Format with `cargo fmt` before committing
- Lint with `cargo clippy` and resolve warnings
- Organize new logic by responsibility: system-level operations go in `services/`, exposed frontend commands go in `commands/`
- Public functions and modules should have doc comments (`///`)
- Avoid `unwrap()` in code paths that can fail from user input or system state — handle errors explicitly (`Result`, `?`)

### Frontend (`src/`)

- Format with Prettier, lint with ESLint (`npm run lint`)
- Use the Composition API (`<script setup>`) for all new Vue components
- Shared logic goes in `composables/`, not duplicated across components
- Type everything — avoid `any` in TypeScript unless justified with a comment
- Keep components focused; if a component exceeds ~200 lines, consider splitting it

### General

- Keep functions small and single-purpose
- Prefer clear naming over comments explaining unclear naming
- No commented-out dead code in committed changes

---

## 📝 Commit Messages

We follow [Conventional Commits](https://www.conventionalcommits.org/):

```
feat: add PHP version switcher UI
fix: resolve port conflict false positive on Nginx
docs: update setup guide for Windows 11
refactor: extract service start logic into separate module
```

Common types: `feat`, `fix`, `docs`, `refactor`, `test`, `chore`.

---

## ✅ Pull Request Process

1. Fork the repo and create your branch from `develop`
2. Make your changes, following the coding conventions above
3. Ensure the app builds and runs locally (`npm run tauri dev`)
4. Run formatters/linters (`cargo fmt`, `cargo clippy`, `npm run lint`)
5. Write a clear PR description: what changed, why, and how to test it
6. Link the related issue (if any) using `Closes #<issue-number>`
7. A maintainer will review your PR — please respond to review comments promptly
8. Once approved, your PR will be merged into `develop`

---

## 🧪 Testing

- New logic in `src-tauri/src/services/` should include unit tests where feasible (`cargo test`)
- For frontend logic in `composables/` or `stores/`, add tests if the logic is non-trivial
- Manual testing steps should be described in your PR if automated tests aren't practical (e.g. UI behavior, OS-level file changes)

---

## 📚 Documentation

If your change affects how the app is used, configured, or built:

- Update the relevant file in `docs/`
- Update `README.md` if it affects setup or top-level usage
- Add inline doc comments for new public functions/modules

Documentation changes are just as valuable as code changes — PRs that only improve docs are welcome.

---

## 🐛 Reporting Bugs

Use the bug report issue template and include:

- Rezure version
- Windows version
- Steps to reproduce
- Expected vs actual behavior
- Relevant logs (from the in-app log viewer, if applicable)

---

## 💡 Suggesting Features

Use the feature request issue template. Describe the problem you're trying to solve, not just the solution — this helps maintainers evaluate fit with the project roadmap (see [`docs/roadmap.md`](docs/roadmap.md)).

---

## 🙋 Questions

If anything is unclear, open a [Discussion](../../discussions) or ask in an issue. We'd rather answer a question than have you guess and submit something that doesn't fit.

Thanks again for helping make Rezure better! 🚀