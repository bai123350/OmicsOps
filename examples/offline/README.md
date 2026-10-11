# Bundled offline example

`synthetic-evidence-report-v1.json` is original OmicsOps content licensed under
AGPL-3.0-only, the repository license. It contains synthetic observations and
preset arithmetic results, not an imported transcript, reasoning trace or sample.
合成数据，预置结果，未在本机执行。
Synthetic data, preset results, not executed on this computer.

The host compiles this manifest and the unmodified root `LICENSE` with
`include_str!`. It adds `LICENSE.txt` and constructs `SOURCE.json` with the hashes
of the other six files; the receipt hashes all seven files. Every output is UTF-8.
Limits: manifest 256 KiB, each file 64 KiB, all files 512 KiB and at most 12 files
(including the license). Only the seven fixed output paths are accepted.

Creating the example publishes a unique local directory and separately registers
an ordinary Blank/Ready Project plus its safe identity marker atomically. No
conversation, run, approval, evidence or Agent event is created. The project ID is
the request UUID; project timestamps use the preset fixture date
2026-10-04T00:00:00Z, not local creation or execution evidence. Library date sorting
may therefore place new examples among older projects. A retry checks the exact
manifest, receipt and files and never repairs user edits. Published files survive
a database registration failure. A fresh request creates a distinct directory.

Directory publication currently supports Windows. The host retains the staging
directory's identity handle, verifies publication and cleanup root handles against
that identity, and publishes with a Win32 handle rename that refuses an existing target.
Cleanup retains directory guards and deletes through those same handles; unknown
or replaced staging trees remain untouched for inspection.
Other hosts return `example_platform_unavailable`; no macOS behavior or package is
claimed as verified. Normal file preview is provided separately by the host's
validated project-file reader. Hashes establish content equality only, not
scientific validity, authorization or successful reproduction.
