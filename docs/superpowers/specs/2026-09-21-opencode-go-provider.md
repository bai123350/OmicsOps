# OpenCode Go provider preset

The Models settings page exposes OpenCode Go as a convenience preset for the official
`https://opencode.ai/zen/go/v1` endpoint. It uses the existing model profile schema and
credential vault: Chat Completions models are stored as `open_ai_compatible`, Anthropic
Messages models are stored as `anthropic`, and the API key remains in Windows Credential
Manager. Known model IDs select their documented protocol exactly. A custom model ID
remains usable with an explicit protocol choice; this does not infer capabilities from a
name or family.

The currently documented Responses-only models are visible but disabled because OmicsOps
does not implement the Responses protocol. The native save boundary rejects those exact
models and rejects a known Chat or Messages model saved under the wrong protocol. Model
discovery still comes from the provider `/models` endpoint. The bundled models.dev snapshot
has no OpenCode Go capability rows, so the UI says that unknown profiles use the existing
conservative legacy context/output budget unless the user supplies an explicit context
budget. No capability, tool, vision, or reasoning support is inferred for this gateway.

Only a base URL whose parsed scheme, host, effective port, path, user information, query,
and fragment exactly match the official endpoint receives the OpenCode headers. Every
probe, model-list request, primary/delegated model call, follow-up request, side chat, and
session review sends the OmicsOps client `User-Agent` and an opaque UUID in
`x-opencode-session`. Runtime clients use the conversation UUID so the value is stable
across turns and related auxiliary calls without transmitting conversation text. Standalone
probe and discovery clients use a random UUID for their client lifetime. Redirects are
disabled for the trusted Go client so these headers cannot be forwarded to another host.

Deterministic tests cover exact URL gating, header injection on POST and GET request paths,
stable and distinct session UUIDs, protocol selection, Responses rejection, editing, and
discovered-model handling. No live OpenCode request was executed because this checkout has
no configured live-model credential/profile; deterministic success does not establish
service availability or account access.
