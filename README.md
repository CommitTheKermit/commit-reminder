# Commit Reminder

A Tauri + React menu-bar utility that scans Git repositories and reminds you when your current changes look ready to commit.

## Features

- Register a parent folder and automatically discover child Git repositories.
- Rule-based reminders using changed line count, file count, and time since the last commit.
- Gemini-based AI judgement using a Google AI Studio API key.
- Sensitive files are excluded before diff content is sent to AI.
- API keys are stored in the OS keychain, not in the config file.
- macOS desktop notifications and tray/menu-bar access.

## Requirements

- Node.js and npm
- Rust toolchain for Tauri (`rustc` and `cargo`)
  - Install from <https://rustup.rs/> if `npx tauri info` reports Rust as missing.
- A Gemini API key from Google AI Studio for AI analysis.

## Development

```bash
npm install
npm run build
npm test
npm run tauri dev
```

## Configuration

The app stores non-secret settings in the OS config directory under `commit-reminder/config.json`.
Gemini API keys are stored through the OS keychain. You can also provide `GEMINI_API_KEY` or `GOOGLE_API_KEY` in the environment during development.

Default safety exclusions include `.env*`, secret/key/certificate-like paths, lock files, build outputs, dependency directories, and large diffs.
