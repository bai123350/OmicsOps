# Settings center verification — 2026-09-21

## Scope and evidence

The implementation adapts the 19 Wisp settings pages to OmicsOps. Privacy is an additional page, so the final navigation has 20 entries. This record distinguishes deterministic checks, browser fixtures, and real native/live acceptance. The Windows manual checklist remains in `2026-09-16-settings-windows-smoke.md`.

## Checks executed by the coordinating agent

- `npm ci`: passed; 304 packages installed. `package-lock.json` unchanged.
- `npm test`: passed; 96 Vitest files / 779 tests and 22 browser-extension tests. Both command stages exited successfully.
- `npm run build`: passed. Vite reports the existing large-chunk advisory; this is not a build failure.
- Final Rust workspace tests, formatting, and desktop build: pending native lifecycle changes at the time of this entry; results will be appended after code freezes.

## Browser checks

Built production assets were served locally through Vite preview and inspected in headless Microsoft Edge. No real model, SSH host, filesystem mutation, keyring operation, or OS notification was exercised by these browser fixtures.

- All 20 navigation entries opened at a physical 720×600 viewport, application scale 120%, explicit dark theme, OS light preference. Every main panel measured `clientWidth = scrollWidth = 410`; physical bounds were x=228 through x=720. Immediate window Escape closed Settings. This checks the empty/browser fallback states, not every native data state.
- Plugins details used a synthetic installed package with 20 skills and long source/relative paths. Physical dialog bounds were x=24, y=26.4, width=672, height=547.2. Body width and scroll width were both 558. Content scrolls, dark colors remain legible, and immediate Escape closes only the details dialog while Settings stays open.
- Skills removal confirmation used a synthetic owned package. Bounds were x=72, y=215, width=576, height=169.9. Immediate Escape closed confirmation and retained details. File preview Escape similarly retained details. The preview close button had light text `rgb(242,244,247)` on dark `rgb(32,42,61)` after the contrast fix.
- Selection actions were checked at 1000×850 and 120% scale with synthetic persisted message text near the viewport's right edge. The fixed toolbar ended at x=992; resizing to 850×700 dismissed it. Previous checks preserved existing draft text when quoting and did not send a message.
- General notification controls used a mock denied OS permission response. The page distinguished the enabled preference from denied system permission and retained an accessible Disable control.

Viewed screenshots are stored outside the repository in the task visualization directory: `settings-plugins-details-ui-fixture.png`, `settings-skills-removal-ui-verified.png`, `settings-selection-toolbar-zoom-fixed.png`, and `settings-general-notifications-ui-fixture.png`. Earlier screenshots showing overflow or poor close-button contrast are superseded and are not completion evidence.

## Real acceptance limits

Real Windows notification delivery/permission, keyring smoke, and installed desktop UI smoke have not been executed by the coordinating agent. Real model/SSH/R/Micromamba/PBMC acceptance is not executed: the required disposable `OMICSOPS_LIVE_*` environment was not configured. Passing mocks, deterministic Rust tests, and builds must not be described as those end-to-end runs.
