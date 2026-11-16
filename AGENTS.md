# Psalm Stage Agent & Contributor Rules

## Documentation & Screenshot Maintenance (Required for New Features)

Whenever you add new capabilities, major UI changes, new workspace views/modals, new hardware/MIDI workflows, or new CLI/MCP commands:

1. **Update User Documentation**
   - Document the new feature or workflow in the appropriate page(s) under `website/src/content/docs/` (or add a new page and register it in `website/astro.config.mjs`).
   - Update `README.md` and technical guides in `docs/` (or `openspec/`) if the feature affects top-level capabilities, installation, hardware wiring, MIDI mapping, or bundle formats.

2. **Update & Capture Screenshots (`scripts/capture-docs-screenshots.mjs`)**
   - For major UI additions, new tabs, or new modals/studios, add or update the capture steps in `scripts/capture-docs-screenshots.mjs` so the new UI is showcased.
   - Re-build the frontend and refresh screenshots + auto-synced docs before finishing:
     ```bash
     npm run build
     node scripts/capture-docs-screenshots.mjs
     node scripts/sync-docs.mjs
     ```
   - Embed any new screenshots (`website/src/assets/screenshots/<name>.png` and `docs/assets/screenshots/<name>.png`) in the relevant `website/src/content/docs/**/*.mdx` pages (and `README.md` when appropriate) and visually verify the PNGs.

3. **Verify Documentation & Site Build**
   ```bash
   node scripts/sync-docs.mjs --check
   cd website && npm run build
   ```

4. **Commit Convention & Release Automation**
   - Use Conventional Commits (`feat:`, `fix:`, `docs:`, `chore:`) so `.github/workflows/release-please.yml` automatically manages version bumps, `CHANGELOG.md`, release tags, and cross-platform Tauri installer builds.
