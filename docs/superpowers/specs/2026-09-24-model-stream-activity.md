# Model stream activity

During a V4 model attempt, the desktop receives transient, payload-free activity for public text, private reasoning, tool-call assembly, and provider retry. Activity carries run and attempt IDs. The UI shows the current phase only for the pending attempt and drops it at a durable completion, pause, or next attempt boundary. The 90-second warning uses the latest matching activity or committed Agent event. A genuinely silent provider still triggers the warning, and the existing absolute model timeout and cancellation checks remain in force.

Reasoning text and partial tool arguments are never sent in activity events, stored in SQLite, or shown as public progress. Completed public text remains a single durable `ModelText` event. Provider retry and usage events are appended to the audit chain as they arrive, preserving request and attempt identity even when the provider stream remains open.

The activity channel is best effort and not audit evidence. On restart, the UI derives its baseline from durable events and waits for new stream packets. This behavior is verified with decoder, in-flight core, DTO contract, and frontend tests; it does not establish that every external model will stream private reasoning packets.
