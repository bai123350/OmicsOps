# Model provider deletion

The Models settings page lets a user delete a saved model profile after an explicit
confirmation. The confirmation is the top window Escape layer: pressing Escape closes it
without closing the settings panel. A failed deletion stays open and shows the backend
error so an active task can be stopped and the operation retried.

Deletion removes the profile and its credential from the existing credential vault. It
also clears references that represent mutable preferences: conversation and Codex turn
selections, delegated-model choices on other profiles, and reviewer defaults. Frozen run,
plan, queue, side-chat, review, and audit JSON remains unchanged. A profile cannot be
deleted while a nonterminal run, pending or executing composer queue item, queued or
running side chat, or running session review has frozen that profile as its primary,
delegated, or reviewer model.

The store performs the reference preflight and database updates in a `BEGIN IMMEDIATE`
transaction. Model save and deletion share the credential-mutation lock. The vault entry
is deleted before the database transaction commits: a vault failure rolls the database
changes back, while the rare case where the database commit fails after vault deletion is
reported explicitly because the credential cannot be restored automatically.

After backend success, the desktop immediately removes the profile from local selection
state and never chooses a replacement automatically. It refreshes profiles and the current
project's conversation selections independently. A refresh failure does not present the
already-completed backend deletion as retryable; the UI reports that the settings or
project should be reopened. Conversation refresh results are ignored if the user has moved
to another project while the request is in flight. An edit form for the deleted profile is
closed, and another open form drops a delegation to the deleted profile.

Automated coverage includes active-reference rejection, terminal-history preservation,
mutable-reference cleanup, credential failure rollback, commit-failure reporting, command
registration, the Tauri API wrapper, confirmation and Escape behavior, error retry,
editing-state cleanup, and desktop refresh behavior. These tests use an in-memory store and
credential vault; no real credential or live model request is required.
