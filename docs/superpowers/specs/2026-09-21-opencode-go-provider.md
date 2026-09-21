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
discovery still comes from the provider `/models` endpoint and supplies IDs rather than token
limits. The bundled models.dev snapshot contains only reviewed OpenCode Go rows, keyed by exact
wire protocol, HTTPS host/port, `/zen/go/v1` base path, and full model ID. The provider's merged
`glm-5.3` row supplies a 1,000,000-token context limit and 131,072-token output limit. Responses
models are excluded. No capability, tool, vision, or reasoning support is inferred from a model
family, a sibling path, or an unknown gateway.

Profiles saved before these rows existed retain their old frozen capability contract. The Models
UI directs the user to edit such a profile, select **Adopt current catalog capabilities on save**,
save it, and start a new conversation. That explicit refresh persists the exact snapshot and
catalog default while retaining an explicitly entered lower context bound. Existing runs and
approved plans are not rewritten.

The catalog is regenerated as one reproducible models.dev snapshot rather than combining source
versions. This refresh therefore also adopts current upstream rows for already supported providers,
including retired model removal, capability updates, and the current MiniMax China endpoint. Saved
profile snapshots remain unchanged unless explicitly refreshed.

The connection probe reserves up to 4,096 output tokens even when the profile has no explicit
reasoning effort, because a gateway model may require reasoning internally. An exact catalog
output limit or smaller request budget can reduce that allowance. The probe succeeds only after
a non-empty, text-only terminal reply; a truncated, unterminated, refused, or tool-call response
remains a failed test and does not weaken normal runtime completion checks.

Only a base URL whose parsed scheme, host, effective port, path, user information, query,
and fragment exactly match the official endpoint receives the OpenCode headers. Every
probe, model-list request, primary/delegated model call, follow-up request, side chat, and
session review sends the OmicsOps client `User-Agent` and an opaque UUID in
`x-opencode-session`. Runtime clients use the conversation UUID so the value is stable
across turns and related auxiliary calls without transmitting conversation text. Standalone
probe and discovery clients use a random UUID for their client lifetime. Redirects are
disabled for the trusted Go client so these headers cannot be forwarded to another host.

Deterministic tests cover exact URL and catalog-path gating, header injection on POST and GET request paths,
stable and distinct session UUIDs, protocol selection, Responses rejection, editing,
discovered-model handling, explicit legacy-profile refresh, full built-in Agent tool-schema budget
preflight, implicit-reasoning probe budgets, terminal responses, and rejection
of truncated or tool-bearing probe replies. On 2026-09-21, the existing ignored live model test
was explicitly run on Windows against a saved OpenCode Go `glm-5.3` profile and passed 1/1 in
5.23 seconds. It resolved the credential through the system vault without printing or writing the
secret and established endpoint authentication plus one complete minimal Chat Completions reply.
It did not exercise a full Agent run, tool calling, SSH, or other OpenCode models.

The separate ignored ordinary-Agent acceptance was also run on Windows on
2026-09-21 with `OMICSOPS_LIVE_MODEL_PROFILE_ID` pointing to that saved profile.
It passed 1/1 in 110.50 seconds through production V4 composition and execution:
two real model requests surrounded one successful `project.read`, the model
returned a random file nonce in its completion answer, and the durable event
chain ended in `RunCompleted`. The live run deliberately exposed only the
read-only file capability plus Agent coordination tools. A deterministic
`DesktopModelPortV4` regression separately covers the full built-in tool schema
and an initial serialized request larger than 150 KB. These checks do not accept
SSH, miRNA analysis, other OpenCode models, or a general scientific workflow.
