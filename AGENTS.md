# Repository Guidelines

## Project Structure & Module Organization

This repository is a Tauri 2 desktop app with a React/Vite frontend.

- `src/`: React UI, shared TypeScript types, and frontend helpers.
- `src/lib/`: frontend utility logic and Vitest tests, e.g. `recommendation.ts` and `recommendation.test.ts`.
- `src-tauri/src/`: Rust backend commands for Git scanning, Gemini calls, keychain access, tray setup, and notifications.
- `src-tauri/icons/`: app icon assets.
- `src-tauri/capabilities/`: Tauri permissions.
- `src-tauri/Info.plist`: macOS bundle metadata, including default notification alert style.
- `dist/`, `node_modules/`, and `src-tauri/target/` are generated and should not be edited manually.

## Build, Test, and Development Commands

Run commands from the repository root unless noted.

- `npm install`: install frontend and Tauri CLI dependencies.
- `npm run dev`: start the Vite frontend only.
- `npm run tauri dev`: run the full desktop app locally.
- `npm test`: run Vitest unit tests.
- `npm run build`: type-check and build the frontend.
- `cd src-tauri && cargo check`: validate Rust code quickly.
- `npm run tauri -- build --bundles app`: build the macOS `.app` bundle.

## Coding Style & Naming Conventions

Use TypeScript `strict` mode and React function components. Prefer small helpers in `src/lib/` for logic that can be tested outside the UI. Use camelCase for TypeScript fields and functions; Rust structs use snake_case with Serde camelCase where exposed to the frontend. Keep indentation at two spaces for TS/JSON/Markdown and four spaces via Rust defaults.

## Testing Guidelines

Vitest is the frontend test framework. Name tests `*.test.ts` and colocate them with the module under test. Add tests for rule, notification, and recommendation logic when behavior changes. For Rust changes, run `cargo check`; add Rust unit tests when backend logic becomes complex or pure enough to isolate.

## Commit & Pull Request Guidelines

Current history uses concise imperative commits, for example `Add agent workflow instructions`. Keep one logical change per commit. Before committing, run relevant checks and inspect `git status`. PRs should include a short summary, test results, linked issues if any, and screenshots for UI changes.

## Security & Configuration Tips

Do not commit API keys or local config. Gemini keys belong in the OS keychain or environment variables such as `GEMINI_API_KEY`. Keep sensitive files excluded from AI diff analysis.

## Agent-Specific Instructions

After each completed feature, bug fix, or configuration change, create a commit. Do not mix unrelated changes in one commit. Push only when the user asks or when a completed work batch should be shared.
