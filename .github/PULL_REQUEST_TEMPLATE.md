## Summary

<!-- Describe what this PR changes and why. -->

## Commit / Release Type

- [ ] `feat:` (new feature — triggers minor bump)
- [ ] `fix:` (bug fix — triggers patch bump)
- [ ] `docs:` / `chore:` / `ci:` / `test:` (no automatic release bump)

## Verification

- [ ] `cargo test --workspace` passes
- [ ] `npx ng test --watch=false` passes
- [ ] `npm run build` succeeds
- [ ] `node scripts/sync-docs.mjs --check` passes
- [ ] `cd website && npm run build` succeeds (if website/docs/UI/CLI changed)
- [ ] User docs (`website/src/content/docs/`) and screenshots (`scripts/capture-docs-screenshots.mjs`) updated for any major UI or feature changes
