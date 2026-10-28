# Settings and Model Experience Implementation Plan

**Goal:** Improve settings readability and make saved model configuration easy to find and edit.

**Architecture:** Preserve React components and existing model callbacks. Change presentation and local interaction state only; use existing appearance tokens and window Escape stack.

**Tech Stack:** React 19, TypeScript, CSS, Vitest / Testing Library, Tauri 2.

**Spec:** `docs/superpowers/specs/2026-10-27-settings-model-experience-design.md`

## Constraints

- No new dependencies, DTO, database migration or credential storage path.
- Preserve exact model/protocol/catalog and budget semantics.
- No real API key, network or SSH dependency for automated tests.
- Existing model save callback selects the saved model; do not claim otherwise.

## Review focus

Pending save followed by editor switch; stale connection/discovery state after edit; omitted credentials on edit; long returned model IDs; immediate Escape without moving focus. Add tests in the model task for these cases.

## Tasks

- [x] Unify shell styling in `src/features/settings/settings.css`: sidebar header, readable navigation, constrained content, common controls, theme tokens, 900px desktop layout. Verify existing navigation tests and browser layout.
- [x] Update `SettingsPanel.tsx`, `model-form.css` and settings tests: write failing tests for searchable/category-filtered catalog, advanced options, key visibility, async save replacement and searchable discovery; implement, then run settings suites. Preserve subscription ownership and all existing contract assertions.
- [x] Inspect diffs independently, perform browser smoke checks with simulated profiles, run production build, Rust workspace tests and full frontend tests; record actual outcomes and unexecuted live acceptance.

## Execution notes

The user requested implementation. Shell styling and model interaction work proceed in parallel in the existing clean workspace, with separate file ownership. No release, distribution or external service mutation is needed.

## Verification results — 2026-10-27, Windows

- `npm run build`: passed. Vite retains its existing large-chunk warning.
- `cargo test --workspace`: passed, 2053 tests passed and 21 ignored. The first restricted run encountered a temporary-directory rename permission error and loopback HTTP fixture failures; the same command passed with the required local process/network permissions. No Rust implementation changed.
- `npm test`: passed, all 130 Vitest files / 1555 tests, plus 22 browser-extension Node tests. The first run during concurrent Rust work hit one existing DesktopApp test's 5-second timeout; the complete rerun passed without changing its test or timeout.
- Focused settings, model-contract, subscription, navigation and Escape tests: 126 passed. New cases cover provider filtering, advanced disclosure, credential visibility, model ID filtering, save feedback and scroll, editor replacement during an async save, and invalidating both completed and late diagnostics after API/subscription changes.
- `git diff --check`: passed.

Focused command:

```powershell
npx vitest run src/features/settings/SettingsPanel.test.tsx src/features/settings/SubscriptionModelForm.test.tsx src/features/settings/modelProviderPresets.test.ts --reporter=dot
```

Browser smoke used the real React settings components with deterministic mock profiles and callbacks in an ignored local preview fixture. Verified light/dark themes at 1100×680, the editor at 900×560 without horizontal overflow, general/appearance navigation, immediate Escape closing only the editor, discovery filtering and selecting the complete model ID, advanced budget editing, disabled fields during saving, and visible success feedback after returning to the saved profiles section. Preview files and screenshots are kept outside tracked source under `.e2e/`.

No live model, SSH, subscription authorization or packaged desktop acceptance was executed. The 21 ignored Rust tests are not claimed as passing acceptance. Native Windows manual smoke should open Settings, search/add/edit a provider, leave a stored key blank while editing, query available models using a disposable authorized profile, and verify that Escape closes only the top editor or deletion confirmation before closing Settings. This change does not alter Tauri configuration, desktop composition or packaging resources.

Subsequent user-requested Windows rebuild: `npm run build:desktop` passed on 2026-10-27, producing `target/release/omicsops-desktop.exe` and `target/release/bundle/nsis/OmicsOps_0.1.0_x64-setup.exe`. The first restricted attempt compiled the application but could not resolve the NSIS download host; rerunning with local network permissions completed packaging. This verifies compilation and packaging, not installation or live-service acceptance. The executable requires its adjacent bundled resource directories; it is not a standalone portable file.
