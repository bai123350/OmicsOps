# OmicsOps Browser Bridge (Manifest V3)

This is a clean-room, dependency-free Manifest V3 extension. It connects only
to the loopback OmicsOps browser bridge:

- `ws://127.0.0.1:18775/v1/session/shared`
- `ws://127.0.0.1:18776/v1/session/workspace`

The checked-in manifest key is fixed. Its Chrome extension ID is
`joifljknpalpoppceknociillogolbnb` (the first 128 bits of SHA-256 of the DER
public key, with hexadecimal nibbles mapped to `a`–`p`). The ID is repeated in
`protocol.js`; changing the key requires changing the Rust bridge's pinned
origin and this constant together. There is no wildcard origin.

## Wire contract

The first frame on each connection is exactly the session hello shape consumed
by the Rust bridge:

```json
{
  "type": "hello",
  "protocol_version": 1,
  "extension_id": "joifljknpalpoppceknociillogolbnb",
  "session": "shared",
  "capabilities": ["tabs", "scan", "search", "screenshot", "downloads", "debugger"],
  "tab_summaries": []
}
```

The server replies with `hello_ack` containing protocol version `1` and the
same session. Commands use `type`, `request_id`, `protocol_version`, `run_id`,
`method`, and `payload`; replies use `request_id`, `ok`, `data`, and a nullable
string `error`.

Supported methods are `web_search`, `web_open_tab`, `web_scan`,
`web_execute_js`, `web_screenshot`, `web_save_assets`, and `close_run_tabs`.
One extension instance owns one explicitly selected session. Shared and
workspace profiles therefore use separate sockets and per-run tab ledgers.

## Safety boundaries

- Only `http:` and `https:` URLs are accepted. `chrome:`, `file:`,
  `javascript:`, `data:`, credentials in URLs, and malformed URLs are blocked.
- `web_scan` waits for a quiet DOM, returns headings/links/forms/text metadata,
  and detects CAPTCHA indicators. It never solves or clicks a CAPTCHA; a scan
  requiring a human returns a recoverable `CAPTCHA_HUMAN_REQUIRED` reply.
- `web_execute_js` is available only for an opened or explicitly adopted tab,
  is size limited, can use the controlled CDP target, and rejects page AI
  prompting/model API calls. No model credentials enter the page.
- `web_save_assets` verifies each source URL against the approved target host,
  generates the staging ID, downloads without overwriting an existing staged
  file, and returns metadata only. The Host copies and hashes the verified
  staged bytes into a project-relative destination.
- `close_run_tabs` closes only tabs owned by the current run ledger.

## Deterministic tests

From the repository root, run:

```text
node --test browser-extension/tests/*.test.js
```

The tests use only Node's built-in `node:test` and `node:assert` modules.
