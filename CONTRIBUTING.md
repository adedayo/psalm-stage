# Contributing to Psalm Stage

Thank you for helping improve **Psalm Stage** for worship teams, musicians, and church audio engineers!

## Ways to Contribute

* **Report bugs** or stage reliability issues via [GitHub Issues](https://github.com/adedayo/psalm-stage/issues).
* **Improve user documentation** in `website/src/content/docs/` or hardware/MIDI guides in `docs/`.
* **Submit code improvements** for the Rust audio core, Tauri bridge, Angular stage UI, or CLI/MCP server.

---

## Development Setup

### Prerequisites
* **Node.js**: v22+ (`node -v`)
* **Rust**: Stable toolchain (`rustup` / `rustc -vV`, edition 2024, MSRV 1.90)
* **Platform Tauri v2 prerequisites**:
  * **macOS**: Xcode Command Line Tools (`xcode-select --install`)
  * **Windows**: Visual Studio C++ Build Tools + WebView2
  * **Linux**: `libwebkit2gtk-4.1-dev`, `libappindicator3-dev`, `librsvg2-dev`, `patchelf`, `libasound2-dev`, `libssl-dev`

### Running Locally

```bash
# Install JavaScript dependencies
npm install

# Run the Tauri desktop app in development mode
npm run tauri dev
```

To preview or edit the documentation website locally:

```bash
cd website
npm install
npm run dev
```

### Verification Before Opening a Pull Request

Every pull request must pass the full workspace test suite and production build:

```bash
# Rust workspace unit tests (psalm-audio-core, psalm-workbench, psalm-stage-cli)
cargo test --workspace

# Angular frontend unit tests
npx ng test --watch=false

# Angular production build check
npm run build
```

If you modified files under `website/`, `src/`, or `crates/psalm-stage-cli/`, also update docs/screenshots and verify:

```bash
# Refresh screenshots (required for major UI changes; add new views to scripts/capture-docs-screenshots.mjs)
npm run build
node scripts/capture-docs-screenshots.mjs

# Sync & verify auto-generated CLI/MCP/version docs and build the website
node scripts/sync-docs.mjs
node scripts/sync-docs.mjs --check
cd website && npm run build
```

---

## Commit Message Convention (Required for Auto-Release)

Psalm Stage uses **Release Please** to automatically generate changelogs, bump versions across all manifests, and tag GitHub releases. Please follow [Conventional Commits](https://www.conventionalcommits.org/) for commit and PR titles:

| Prefix | Effect on Next Release | Example |
| :--- | :--- | :--- |
| `fix:` | Patch bump (`0.1.0` -> `0.1.1`) | `fix: prevent click drift after rapid loop cancel` |
| `feat:` | Minor bump (`0.1.0` -> `0.2.0`) | `feat: add 6/8 percussion groove presets` |
| `feat!:` or `BREAKING CHANGE:` | Major bump (`0.1.0` -> `1.0.0`) | `feat!: update setlist manifest v2 schema` |
| `docs:`, `chore:`, `ci:`, `test:` | Included or omitted per config without forced release | `docs: clarify Behringer X32 USB routing` |

---

## Licensing

By contributing to Psalm Stage, you agree that your contributions will be licensed under the project's [BSD 3-Clause License](LICENSE).
