-- OmicsOps schema v4.  The database stores durable identifiers, provenance,
-- and references; credentials are held by the platform keyring.

CREATE TABLE IF NOT EXISTS connections (
    id TEXT PRIMARY KEY CHECK (length(trim(id)) > 0),
    label TEXT NOT NULL CHECK (length(trim(label)) > 0),
    profile_json TEXT NOT NULL,
    created_at INTEGER NOT NULL DEFAULT 0,
    updated_at INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS model_profiles (
    id TEXT PRIMARY KEY CHECK (length(trim(id)) > 0),
    value_json TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS skill_packages (
    id TEXT PRIMARY KEY CHECK (length(trim(id)) > 0),
    value_json TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS projects (
    id TEXT PRIMARY KEY CHECK (length(trim(id)) > 0),
    name TEXT NOT NULL CHECK (length(trim(name)) > 0),
    description TEXT NOT NULL DEFAULT '',
    workspace_dir TEXT NOT NULL CHECK (
        length(trim(replace(replace(replace(workspace_dir, char(9), ''), char(10), ''), char(13), ''))) > 0
    ),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS project_omicsops (
    project_id TEXT PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
    local_root TEXT NOT NULL CHECK (
        length(trim(replace(replace(replace(local_root, char(9), ''), char(10), ''), char(13), ''))) > 0
    ),
    remote_root TEXT CHECK (
        remote_root IS NULL OR
        length(trim(replace(replace(replace(remote_root, char(9), ''), char(10), ''), char(13), ''))) > 0
    ),
    connection_id TEXT REFERENCES connections(id) ON DELETE SET NULL,
    template TEXT NOT NULL,
    status TEXT NOT NULL,
    ollama_only INTEGER NOT NULL DEFAULT 0 CHECK (ollama_only IN (0, 1)),
    updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS folders (
    id TEXT PRIMARY KEY CHECK (length(trim(id)) > 0),
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    parent_id TEXT REFERENCES folders(id) ON DELETE CASCADE,
    name TEXT NOT NULL CHECK (length(trim(name)) > 0),
    relative_path TEXT NOT NULL CHECK (length(trim(relative_path)) > 0),
    created_at INTEGER NOT NULL DEFAULT 0,
    updated_at INTEGER NOT NULL DEFAULT 0,
    UNIQUE (project_id, relative_path)
);

CREATE TABLE IF NOT EXISTS frames (
    id TEXT PRIMARY KEY CHECK (length(trim(id)) > 0),
    parent_frame_id TEXT REFERENCES frames(id) ON DELETE SET NULL,
    root_frame_id TEXT REFERENCES frames(id) ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED,
    agent_name TEXT NOT NULL CHECK (length(trim(agent_name)) > 0),
    status TEXT NOT NULL CHECK (length(trim(status)) > 0),
    project_id TEXT REFERENCES projects(id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS conversation_records (
    frame_id TEXT PRIMARY KEY REFERENCES frames(id) ON DELETE CASCADE,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    title TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL CHECK (length(trim(status)) > 0),
    model_profile_id TEXT REFERENCES model_profiles(id) ON DELETE SET NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS messages (
    id TEXT PRIMARY KEY CHECK (length(trim(id)) > 0),
    frame_id TEXT NOT NULL REFERENCES frames(id) ON DELETE CASCADE,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    conversation_id TEXT NOT NULL REFERENCES conversation_records(frame_id) ON DELETE CASCADE,
    seq INTEGER NOT NULL CHECK (seq >= 0),
    role TEXT NOT NULL CHECK (length(trim(role)) > 0),
    content TEXT NOT NULL,
    ts INTEGER NOT NULL,
    CHECK (frame_id = conversation_id),
    UNIQUE (frame_id, seq)
);

CREATE TABLE IF NOT EXISTS session_branch_merges (
    id TEXT PRIMARY KEY CHECK (length(trim(id)) > 0),
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    source_frame_id TEXT NOT NULL REFERENCES frames(id) ON DELETE CASCADE,
    target_frame_id TEXT NOT NULL REFERENCES frames(id) ON DELETE CASCADE,
    merged_at INTEGER NOT NULL,
    value_json TEXT NOT NULL DEFAULT '{}'
);

CREATE TABLE IF NOT EXISTS session_reviews (
    id TEXT PRIMARY KEY CHECK (length(trim(id)) > 0),
    frame_id TEXT NOT NULL REFERENCES frames(id) ON DELETE CASCADE,
    reviewer TEXT NOT NULL CHECK (length(trim(reviewer)) > 0),
    status TEXT NOT NULL,
    value_json TEXT NOT NULL DEFAULT '{}',
    created_at INTEGER NOT NULL DEFAULT 0,
    updated_at INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS session_ui_events (
    id TEXT PRIMARY KEY CHECK (length(trim(id)) > 0),
    frame_id TEXT REFERENCES frames(id) ON DELETE CASCADE,
    event_kind TEXT NOT NULL CHECK (length(trim(event_kind)) > 0),
    value_json TEXT NOT NULL DEFAULT '{}',
    occurred_at INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS artifacts (
    id TEXT PRIMARY KEY CHECK (length(trim(id)) > 0),
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    root_frame_id TEXT REFERENCES frames(id) ON DELETE SET NULL,
    filename TEXT NOT NULL CHECK (length(trim(filename)) > 0),
    content_type TEXT NOT NULL DEFAULT 'application/octet-stream',
    storage_path TEXT NOT NULL CHECK (length(trim(storage_path)) > 0),
    created_at INTEGER NOT NULL,
    logical_key TEXT,
    source_run_id TEXT REFERENCES runs(id) ON DELETE SET NULL,
    remote_path TEXT,
    size_bytes INTEGER NOT NULL DEFAULT 0 CHECK (size_bytes >= 0),
    sha256 TEXT NOT NULL DEFAULT '',
    verified INTEGER NOT NULL DEFAULT 0 CHECK (verified IN (0, 1))
);

CREATE TABLE IF NOT EXISTS artifact_versions (
    id TEXT PRIMARY KEY CHECK (length(trim(id)) > 0),
    artifact_id TEXT NOT NULL REFERENCES artifacts(id) ON DELETE CASCADE,
    version INTEGER NOT NULL CHECK (version > 0),
    storage_path TEXT NOT NULL CHECK (length(trim(storage_path)) > 0),
    size_bytes INTEGER NOT NULL DEFAULT 0 CHECK (size_bytes >= 0),
    sha256 TEXT NOT NULL DEFAULT '',
    created_at INTEGER NOT NULL DEFAULT 0,
    UNIQUE (artifact_id, version)
);

CREATE TABLE IF NOT EXISTS artifact_dependencies (
    artifact_id TEXT NOT NULL REFERENCES artifacts(id) ON DELETE CASCADE,
    dependency_artifact_id TEXT NOT NULL REFERENCES artifacts(id) ON DELETE CASCADE,
    relation TEXT NOT NULL DEFAULT 'derived_from',
    created_at INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (artifact_id, dependency_artifact_id),
    CHECK (artifact_id <> dependency_artifact_id)
);

CREATE TABLE IF NOT EXISTS message_resource_links (
    message_id TEXT NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    resource_id TEXT NOT NULL,
    resource_kind TEXT NOT NULL CHECK (length(trim(resource_kind)) > 0),
    created_at INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (message_id, resource_id, resource_kind)
);

CREATE TABLE IF NOT EXISTS turn_file_undo (
    id TEXT PRIMARY KEY CHECK (length(trim(id)) > 0),
    frame_id TEXT NOT NULL REFERENCES frames(id) ON DELETE CASCADE,
    turn_id TEXT NOT NULL,
    path TEXT NOT NULL CHECK (length(trim(path)) > 0),
    previous_content TEXT,
    previous_sha256 TEXT,
    created_at INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS settings (
    scope TEXT NOT NULL DEFAULT 'global',
    key TEXT NOT NULL CHECK (length(trim(key)) > 0),
    value_json TEXT NOT NULL,
    updated_at INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (scope, key)
);

CREATE TABLE IF NOT EXISTS proposed_plans (
    id TEXT PRIMARY KEY CHECK (length(trim(id)) > 0),
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    frame_id TEXT NOT NULL REFERENCES frames(id) ON DELETE CASCADE,
    revision INTEGER NOT NULL CHECK (revision > 0),
    plan_hash TEXT NOT NULL CHECK (length(trim(plan_hash)) > 0),
    status TEXT NOT NULL CHECK (length(trim(status)) > 0),
    plan_json TEXT NOT NULL,
    markdown TEXT NOT NULL DEFAULT '',
    feedback TEXT,
    run_id TEXT NOT NULL REFERENCES agent_runs_v4(run_id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL DEFAULT 0,
    updated_at INTEGER NOT NULL DEFAULT 0,
    UNIQUE (frame_id, revision),
    UNIQUE (frame_id, plan_hash)
);

-- Plan content is append-only. Lifecycle metadata (status, feedback, and
-- updated_at) is intentionally mutable through Store transactions, while a
-- direct SQL caller cannot rewrite the identity, revision, hash, structured
-- plan, Markdown, owner, run, or creation timestamp of an existing proposal.
CREATE TRIGGER IF NOT EXISTS trg_proposed_plans_immutable_content
BEFORE UPDATE OF status,id,project_id,frame_id,revision,plan_hash,plan_json,markdown,run_id,created_at
ON proposed_plans
WHEN NOT (
    NEW.id IS OLD.id
    AND NEW.project_id IS OLD.project_id
    AND NEW.frame_id IS OLD.frame_id
    AND NEW.revision IS OLD.revision
    AND NEW.plan_hash IS OLD.plan_hash
    AND NEW.plan_json IS OLD.plan_json
    AND NEW.markdown IS OLD.markdown
    AND NEW.run_id IS OLD.run_id
    AND NEW.created_at IS OLD.created_at
    AND (
        NEW.status IS OLD.status
        OR (OLD.status = 'generating' AND NEW.status IN ('revising','cancelled','superseded'))
        OR (OLD.status = 'revising' AND NEW.status IN ('generating','cancelled','superseded'))
        OR (OLD.status = 'pending' AND NEW.status IN ('approved','revising','cancelled','superseded'))
        OR (OLD.status = 'approved' AND NEW.status = 'superseded')
    )
)
BEGIN
    SELECT RAISE(ABORT, 'proposed plan revision content is immutable');
END;

CREATE TABLE IF NOT EXISTS codex_turn_configs (
    frame_id TEXT PRIMARY KEY REFERENCES frames(id) ON DELETE CASCADE,
    model_profile_id TEXT REFERENCES model_profiles(id) ON DELETE SET NULL,
    mode TEXT NOT NULL DEFAULT 'agent',
    config_json TEXT NOT NULL DEFAULT '{}',
    updated_at INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS acp_sessions (
    id TEXT PRIMARY KEY CHECK (length(trim(id)) > 0),
    project_id TEXT REFERENCES projects(id) ON DELETE CASCADE,
    frame_id TEXT REFERENCES frames(id) ON DELETE SET NULL,
    endpoint TEXT NOT NULL CHECK (length(trim(endpoint)) > 0),
    status TEXT NOT NULL,
    metadata_json TEXT NOT NULL DEFAULT '{}',
    created_at INTEGER NOT NULL DEFAULT 0,
    updated_at INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS codex_imports (
    id TEXT PRIMARY KEY CHECK (length(trim(id)) > 0),
    project_id TEXT REFERENCES projects(id) ON DELETE CASCADE,
    source_path TEXT NOT NULL CHECK (length(trim(source_path)) > 0),
    source_digest TEXT NOT NULL DEFAULT '',
    imported_count INTEGER NOT NULL DEFAULT 0 CHECK (imported_count >= 0),
    status TEXT NOT NULL,
    value_json TEXT NOT NULL DEFAULT '{}',
    created_at INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS external_session_cache (
    provider TEXT NOT NULL,
    external_id TEXT NOT NULL,
    project_id TEXT REFERENCES projects(id) ON DELETE CASCADE,
    frame_id TEXT REFERENCES frames(id) ON DELETE SET NULL,
    payload_json TEXT NOT NULL,
    fetched_at INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (provider, external_id)
);

CREATE TABLE IF NOT EXISTS execution_contexts (
    id TEXT PRIMARY KEY CHECK (length(trim(id)) > 0),
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (length(trim(kind)) > 0),
    label TEXT NOT NULL CHECK (length(trim(label)) > 0),
    location TEXT NOT NULL CHECK (length(trim(location)) > 0),
    status TEXT NOT NULL,
    config_json TEXT NOT NULL DEFAULT '{}',
    created_at INTEGER NOT NULL DEFAULT 0,
    updated_at INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS session_execution_contexts (
    frame_id TEXT NOT NULL REFERENCES frames(id) ON DELETE CASCADE,
    execution_context_id TEXT NOT NULL REFERENCES execution_contexts(id) ON DELETE CASCADE,
    attached_at INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (frame_id, execution_context_id)
);

CREATE TABLE IF NOT EXISTS context_storage_prefs (
    project_id TEXT PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
    mode TEXT NOT NULL DEFAULT 'local',
    root_path TEXT,
    remote_uri TEXT,
    updated_at INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS remote_staging (
    id TEXT PRIMARY KEY CHECK (length(trim(id)) > 0),
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    execution_context_id TEXT REFERENCES execution_contexts(id) ON DELETE SET NULL,
    local_path TEXT NOT NULL CHECK (length(trim(local_path)) > 0),
    remote_path TEXT NOT NULL CHECK (length(trim(remote_path)) > 0),
    state TEXT NOT NULL,
    size_bytes INTEGER NOT NULL DEFAULT 0 CHECK (size_bytes >= 0),
    sha256 TEXT NOT NULL DEFAULT '',
    updated_at INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS runs (
    id TEXT PRIMARY KEY CHECK (length(trim(id)) > 0),
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    frame_id TEXT REFERENCES frames(id) ON DELETE SET NULL,
    execution_context_id TEXT REFERENCES execution_contexts(id) ON DELETE SET NULL,
    status TEXT NOT NULL CHECK (length(trim(status)) > 0),
    run_kind TEXT NOT NULL DEFAULT 'analysis',
    started_at INTEGER,
    finished_at INTEGER,
    value_json TEXT NOT NULL DEFAULT '{}',
    created_at INTEGER NOT NULL DEFAULT 0,
    updated_at INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS run_artifacts (
    run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    artifact_id TEXT NOT NULL REFERENCES artifacts(id) ON DELETE CASCADE,
    role TEXT NOT NULL DEFAULT 'output',
    created_at INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (run_id, artifact_id, role)
);

CREATE TABLE IF NOT EXISTS external_resources (
    id TEXT PRIMARY KEY CHECK (length(trim(id)) > 0),
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    resource_kind TEXT NOT NULL,
    locator TEXT NOT NULL CHECK (length(trim(locator)) > 0),
    checksum TEXT,
    metadata_json TEXT NOT NULL DEFAULT '{}',
    created_at INTEGER NOT NULL DEFAULT 0,
    updated_at INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS run_inputs (
    run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    input_key TEXT NOT NULL,
    resource_id TEXT,
    artifact_id TEXT REFERENCES artifacts(id) ON DELETE SET NULL,
    value_json TEXT NOT NULL DEFAULT '{}',
    PRIMARY KEY (run_id, input_key)
);

CREATE TABLE IF NOT EXISTS run_outputs (
    run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    output_key TEXT NOT NULL,
    artifact_id TEXT REFERENCES artifacts(id) ON DELETE SET NULL,
    value_json TEXT NOT NULL DEFAULT '{}',
    PRIMARY KEY (run_id, output_key)
);

CREATE TABLE IF NOT EXISTS run_code_snapshots (
    id TEXT PRIMARY KEY CHECK (length(trim(id)) > 0),
    run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    source_path TEXT NOT NULL CHECK (length(trim(source_path)) > 0),
    content TEXT NOT NULL,
    sha256 TEXT NOT NULL DEFAULT '',
    created_at INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS env_snapshots (
    id TEXT PRIMARY KEY CHECK (length(trim(id)) > 0),
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    execution_context_id TEXT REFERENCES execution_contexts(id) ON DELETE SET NULL,
    platform TEXT NOT NULL,
    interpreter TEXT,
    packages_json TEXT NOT NULL DEFAULT '{}',
    variables_json TEXT NOT NULL DEFAULT '{}',
    digest TEXT NOT NULL DEFAULT '',
    created_at INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS run_environment_snapshots (
    run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    env_snapshot_id TEXT NOT NULL REFERENCES env_snapshots(id) ON DELETE RESTRICT,
    role TEXT NOT NULL DEFAULT 'runtime',
    PRIMARY KEY (run_id, env_snapshot_id)
);

CREATE TABLE IF NOT EXISTS run_events (
    id TEXT PRIMARY KEY CHECK (length(trim(id)) > 0),
    run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    sequence INTEGER NOT NULL CHECK (sequence > 0),
    event_kind TEXT NOT NULL,
    value_json TEXT NOT NULL,
    occurred_at INTEGER NOT NULL DEFAULT 0,
    UNIQUE (run_id, sequence)
);

CREATE TABLE IF NOT EXISTS publications (
    id TEXT PRIMARY KEY CHECK (length(trim(id)) > 0),
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    title TEXT NOT NULL CHECK (length(trim(title)) > 0),
    status TEXT NOT NULL,
    created_at INTEGER NOT NULL DEFAULT 0,
    updated_at INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS publication_revisions (
    id TEXT PRIMARY KEY CHECK (length(trim(id)) > 0),
    publication_id TEXT NOT NULL REFERENCES publications(id) ON DELETE CASCADE,
    revision INTEGER NOT NULL CHECK (revision > 0),
    content TEXT NOT NULL,
    content_sha256 TEXT NOT NULL DEFAULT '',
    created_at INTEGER NOT NULL DEFAULT 0,
    UNIQUE (publication_id, revision)
);

CREATE TABLE IF NOT EXISTS publication_items (
    id TEXT PRIMARY KEY CHECK (length(trim(id)) > 0),
    publication_id TEXT NOT NULL REFERENCES publications(id) ON DELETE CASCADE,
    item_kind TEXT NOT NULL,
    title TEXT NOT NULL DEFAULT '',
    position INTEGER NOT NULL DEFAULT 0 CHECK (position >= 0),
    value_json TEXT NOT NULL DEFAULT '{}',
    created_at INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS publication_item_links (
    publication_item_id TEXT NOT NULL REFERENCES publication_items(id) ON DELETE CASCADE,
    artifact_id TEXT REFERENCES artifacts(id) ON DELETE SET NULL,
    evidence_id TEXT,
    relation TEXT NOT NULL DEFAULT 'supports',
    PRIMARY KEY (publication_item_id, artifact_id, evidence_id)
);

CREATE TABLE IF NOT EXISTS evidence_bindings (
    id TEXT PRIMARY KEY CHECK (length(trim(id)) > 0),
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    publication_id TEXT REFERENCES publications(id) ON DELETE CASCADE,
    publication_item_id TEXT REFERENCES publication_items(id) ON DELETE CASCADE,
    artifact_id TEXT REFERENCES artifacts(id) ON DELETE SET NULL,
    source_kind TEXT NOT NULL,
    source_id TEXT NOT NULL,
    excerpt TEXT NOT NULL DEFAULT '',
    created_at INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS evidence_reviews (
    id TEXT PRIMARY KEY CHECK (length(trim(id)) > 0),
    evidence_binding_id TEXT NOT NULL REFERENCES evidence_bindings(id) ON DELETE CASCADE,
    reviewer TEXT NOT NULL,
    verdict TEXT NOT NULL,
    notes TEXT NOT NULL DEFAULT '',
    reviewed_at INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS evidence_supersessions (
    superseded_id TEXT NOT NULL REFERENCES evidence_bindings(id) ON DELETE CASCADE,
    successor_id TEXT NOT NULL REFERENCES evidence_bindings(id) ON DELETE CASCADE,
    reason TEXT NOT NULL DEFAULT '',
    created_at INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (superseded_id, successor_id),
    CHECK (superseded_id <> successor_id)
);

CREATE TABLE IF NOT EXISTS publication_readiness_reports (
    id TEXT PRIMARY KEY CHECK (length(trim(id)) > 0),
    publication_id TEXT NOT NULL REFERENCES publications(id) ON DELETE CASCADE,
    status TEXT NOT NULL,
    report_json TEXT NOT NULL,
    generated_at INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS publication_waivers (
    id TEXT PRIMARY KEY CHECK (length(trim(id)) > 0),
    publication_id TEXT NOT NULL REFERENCES publications(id) ON DELETE CASCADE,
    criterion TEXT NOT NULL,
    reason TEXT NOT NULL,
    approved_by TEXT NOT NULL,
    created_at INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS publication_freeze_attempts (
    id TEXT PRIMARY KEY CHECK (length(trim(id)) > 0),
    publication_id TEXT NOT NULL REFERENCES publications(id) ON DELETE CASCADE,
    revision INTEGER NOT NULL CHECK (revision > 0),
    status TEXT NOT NULL,
    error TEXT,
    attempted_at INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS capsule_builds (
    id TEXT PRIMARY KEY CHECK (length(trim(id)) > 0),
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    publication_id TEXT REFERENCES publications(id) ON DELETE SET NULL,
    artifact_id TEXT REFERENCES artifacts(id) ON DELETE SET NULL,
    manifest_json TEXT NOT NULL DEFAULT '{}',
    status TEXT NOT NULL,
    created_at INTEGER NOT NULL DEFAULT 0,
    finished_at INTEGER
);

CREATE TABLE IF NOT EXISTS reproduction_runs (
    id TEXT PRIMARY KEY CHECK (length(trim(id)) > 0),
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    capsule_build_id TEXT REFERENCES capsule_builds(id) ON DELETE SET NULL,
    execution_context_id TEXT REFERENCES execution_contexts(id) ON DELETE SET NULL,
    status TEXT NOT NULL,
    started_at INTEGER,
    finished_at INTEGER,
    value_json TEXT NOT NULL DEFAULT '{}'
);

CREATE TABLE IF NOT EXISTS reproduction_results (
    id TEXT PRIMARY KEY CHECK (length(trim(id)) > 0),
    reproduction_run_id TEXT NOT NULL REFERENCES reproduction_runs(id) ON DELETE CASCADE,
    result_kind TEXT NOT NULL,
    passed INTEGER NOT NULL DEFAULT 0 CHECK (passed IN (0, 1)),
    details_json TEXT NOT NULL DEFAULT '{}',
    created_at INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS research_nodes (
    id TEXT PRIMARY KEY CHECK (length(trim(id)) > 0),
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    node_kind TEXT NOT NULL,
    label TEXT NOT NULL DEFAULT '',
    value_json TEXT NOT NULL DEFAULT '{}',
    created_at INTEGER NOT NULL DEFAULT 0,
    updated_at INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS research_edges (
    id TEXT PRIMARY KEY CHECK (length(trim(id)) > 0),
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    source_node_id TEXT NOT NULL REFERENCES research_nodes(id) ON DELETE CASCADE,
    target_node_id TEXT NOT NULL REFERENCES research_nodes(id) ON DELETE CASCADE,
    edge_kind TEXT NOT NULL,
    value_json TEXT NOT NULL DEFAULT '{}',
    created_at INTEGER NOT NULL DEFAULT 0,
    UNIQUE (source_node_id, target_node_id, edge_kind)
);

CREATE TABLE IF NOT EXISTS project_state_counters (
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    counter_key TEXT NOT NULL,
    counter_value INTEGER NOT NULL DEFAULT 0,
    updated_at INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (project_id, counter_key)
);

CREATE TABLE IF NOT EXISTS workspace_snapshots (
    id TEXT PRIMARY KEY CHECK (length(trim(id)) > 0),
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    workspace_dir TEXT NOT NULL CHECK (length(trim(workspace_dir)) > 0),
    digest TEXT NOT NULL DEFAULT '',
    manifest_json TEXT NOT NULL DEFAULT '{}',
    created_at INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS context_archives (
    id TEXT PRIMARY KEY CHECK (length(trim(id)) > 0),
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    frame_id TEXT REFERENCES frames(id) ON DELETE SET NULL,
    through_sequence INTEGER NOT NULL DEFAULT 0 CHECK (through_sequence >= 0),
    transcript TEXT NOT NULL,
    checkpoint_json TEXT NOT NULL DEFAULT '{}',
    created_at INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS project_state_revisions (
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    revision INTEGER NOT NULL CHECK (revision > 0),
    state_hash TEXT NOT NULL,
    state_json TEXT NOT NULL,
    created_at INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (project_id, revision)
);

CREATE TABLE IF NOT EXISTS global_memories (
    id TEXT PRIMARY KEY CHECK (length(trim(id)) > 0),
    project_id TEXT REFERENCES projects(id) ON DELETE CASCADE,
    dimension TEXT NOT NULL,
    memory_key TEXT NOT NULL,
    statement TEXT NOT NULL,
    value_json TEXT NOT NULL DEFAULT '{}',
    created_at INTEGER NOT NULL DEFAULT 0,
    UNIQUE (project_id, dimension, memory_key)
);

CREATE TABLE IF NOT EXISTS exploration_families (
    id TEXT PRIMARY KEY CHECK (length(trim(id)) > 0),
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name TEXT NOT NULL CHECK (length(trim(name)) > 0),
    status TEXT NOT NULL,
    value_json TEXT NOT NULL DEFAULT '{}',
    created_at INTEGER NOT NULL DEFAULT 0,
    updated_at INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS exploration_checkpoints (
    id TEXT PRIMARY KEY CHECK (length(trim(id)) > 0),
    family_id TEXT NOT NULL REFERENCES exploration_families(id) ON DELETE CASCADE,
    sequence INTEGER NOT NULL CHECK (sequence >= 0),
    state_json TEXT NOT NULL DEFAULT '{}',
    created_at INTEGER NOT NULL DEFAULT 0,
    UNIQUE (family_id, sequence)
);

CREATE TABLE IF NOT EXISTS explorations (
    id TEXT PRIMARY KEY CHECK (length(trim(id)) > 0),
    family_id TEXT NOT NULL REFERENCES exploration_families(id) ON DELETE CASCADE,
    status TEXT NOT NULL,
    hypothesis TEXT NOT NULL DEFAULT '',
    value_json TEXT NOT NULL DEFAULT '{}',
    created_at INTEGER NOT NULL DEFAULT 0,
    updated_at INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS exploration_baseline_entities (
    exploration_id TEXT NOT NULL REFERENCES explorations(id) ON DELETE CASCADE,
    entity_key TEXT NOT NULL,
    entity_json TEXT NOT NULL DEFAULT '{}',
    PRIMARY KEY (exploration_id, entity_key)
);

CREATE TABLE IF NOT EXISTS exploration_baseline_artifact_heads (
    exploration_id TEXT NOT NULL REFERENCES explorations(id) ON DELETE CASCADE,
    artifact_id TEXT NOT NULL REFERENCES artifacts(id) ON DELETE CASCADE,
    version_id TEXT,
    captured_at INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (exploration_id, artifact_id)
);

CREATE TABLE IF NOT EXISTS artifact_heads (
    artifact_id TEXT PRIMARY KEY REFERENCES artifacts(id) ON DELETE CASCADE,
    version_id TEXT,
    updated_at INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS exploration_effects (
    id TEXT PRIMARY KEY CHECK (length(trim(id)) > 0),
    exploration_id TEXT NOT NULL REFERENCES explorations(id) ON DELETE CASCADE,
    artifact_id TEXT REFERENCES artifacts(id) ON DELETE SET NULL,
    effect_kind TEXT NOT NULL,
    value_json TEXT NOT NULL DEFAULT '{}',
    created_at INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS exploration_promotions (
    id TEXT PRIMARY KEY CHECK (length(trim(id)) > 0),
    exploration_id TEXT NOT NULL REFERENCES explorations(id) ON DELETE CASCADE,
    target_kind TEXT NOT NULL,
    target_id TEXT NOT NULL,
    status TEXT NOT NULL,
    value_json TEXT NOT NULL DEFAULT '{}',
    created_at INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS project_sync_state (
    project_id TEXT PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
    remote_project_id TEXT,
    last_revision INTEGER NOT NULL DEFAULT 0 CHECK (last_revision >= 0),
    state TEXT NOT NULL DEFAULT 'idle',
    updated_at INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS app_objects (
    kind TEXT NOT NULL CHECK (length(trim(kind)) > 0),
    id TEXT NOT NULL CHECK (length(trim(id)) > 0),
    value_json TEXT NOT NULL,
    updated_at INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (kind, id)
);

CREATE TABLE IF NOT EXISTS approved_plans (
    id TEXT PRIMARY KEY CHECK (length(trim(id)) > 0),
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    frame_id TEXT NOT NULL REFERENCES frames(id) ON DELETE CASCADE,
    plan_hash TEXT NOT NULL,
    approved_at INTEGER NOT NULL DEFAULT 0,
    value_json TEXT NOT NULL DEFAULT '{}',
    UNIQUE (frame_id, plan_hash)
);

CREATE TABLE IF NOT EXISTS audit_events_v2 (
    id TEXT PRIMARY KEY CHECK (length(trim(id)) > 0),
    project_id TEXT REFERENCES projects(id) ON DELETE CASCADE,
    frame_id TEXT REFERENCES frames(id) ON DELETE CASCADE,
    actor TEXT NOT NULL DEFAULT 'system',
    action TEXT NOT NULL,
    value_json TEXT NOT NULL DEFAULT '{}',
    occurred_at INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS step_attempts_v2 (
    id TEXT PRIMARY KEY CHECK (length(trim(id)) > 0),
    run_id TEXT REFERENCES runs(id) ON DELETE CASCADE,
    project_id TEXT REFERENCES projects(id) ON DELETE CASCADE,
    step_key TEXT NOT NULL,
    attempt INTEGER NOT NULL CHECK (attempt > 0),
    status TEXT NOT NULL,
    value_json TEXT NOT NULL DEFAULT '{}',
    started_at INTEGER,
    finished_at INTEGER,
    UNIQUE (run_id, step_key, attempt)
);

CREATE TABLE IF NOT EXISTS environment_locks_v2 (
    id TEXT PRIMARY KEY CHECK (length(trim(id)) > 0),
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    execution_context_id TEXT NOT NULL REFERENCES execution_contexts(id) ON DELETE CASCADE,
    owner TEXT NOT NULL,
    lock_state TEXT NOT NULL,
    acquired_at INTEGER NOT NULL DEFAULT 0,
    released_at INTEGER
);

CREATE TABLE IF NOT EXISTS agent_turns (
    id TEXT PRIMARY KEY CHECK (length(trim(id)) > 0),
    project_id TEXT REFERENCES projects(id) ON DELETE CASCADE,
    frame_id TEXT REFERENCES frames(id) ON DELETE CASCADE,
    status TEXT NOT NULL DEFAULT 'active',
    value_json TEXT NOT NULL DEFAULT '{}',
    created_at INTEGER NOT NULL DEFAULT 0,
    updated_at INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS agent_events (
    id TEXT PRIMARY KEY CHECK (length(trim(id)) > 0),
    turn_id TEXT REFERENCES agent_turns(id) ON DELETE CASCADE,
    project_id TEXT REFERENCES projects(id) ON DELETE CASCADE,
    sequence INTEGER NOT NULL CHECK (sequence > 0),
    event_kind TEXT NOT NULL,
    value_json TEXT NOT NULL,
    occurred_at INTEGER NOT NULL DEFAULT 0,
    UNIQUE (turn_id, sequence)
);

CREATE TABLE IF NOT EXISTS tool_calls (
    id TEXT PRIMARY KEY CHECK (length(trim(id)) > 0),
    turn_id TEXT REFERENCES agent_turns(id) ON DELETE CASCADE,
    project_id TEXT REFERENCES projects(id) ON DELETE CASCADE,
    tool_id TEXT NOT NULL,
    arguments_json TEXT NOT NULL DEFAULT '{}',
    result_json TEXT,
    status TEXT NOT NULL,
    created_at INTEGER NOT NULL DEFAULT 0,
    finished_at INTEGER
);

CREATE TABLE IF NOT EXISTS approvals_v3 (
    id TEXT PRIMARY KEY CHECK (length(trim(id)) > 0),
    project_id TEXT REFERENCES projects(id) ON DELETE CASCADE,
    frame_id TEXT REFERENCES frames(id) ON DELETE CASCADE,
    tool_call_id TEXT REFERENCES tool_calls(id) ON DELETE SET NULL,
    decision TEXT NOT NULL,
    request_hash TEXT NOT NULL DEFAULT '',
    value_json TEXT NOT NULL DEFAULT '{}',
    created_at INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS notebook_entries (
    id TEXT PRIMARY KEY CHECK (length(trim(id)) > 0),
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    conversation_id TEXT REFERENCES conversation_records(frame_id) ON DELETE SET NULL,
    turn_id TEXT,
    kind TEXT NOT NULL,
    title TEXT NOT NULL DEFAULT '',
    markdown TEXT NOT NULL DEFAULT '',
    confidence REAL,
    evidence_json TEXT NOT NULL DEFAULT '[]',
    artifact_ids_json TEXT NOT NULL DEFAULT '[]',
    created_at INTEGER NOT NULL DEFAULT 0,
    updated_at INTEGER NOT NULL DEFAULT 0,
    value_json TEXT NOT NULL DEFAULT '{}'
);

CREATE TABLE IF NOT EXISTS sync_entries (
    id TEXT PRIMARY KEY CHECK (length(trim(id)) > 0),
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    relative_path TEXT NOT NULL CHECK (length(trim(relative_path)) > 0),
    value_json TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS artifacts_v3 (
    id TEXT PRIMARY KEY CHECK (length(trim(id)) > 0),
    project_id TEXT,
    run_id TEXT,
    relative_path TEXT,
    media_type TEXT,
    size_bytes INTEGER,
    sha256 TEXT,
    verified INTEGER,
    value_json TEXT NOT NULL DEFAULT '{}'
);

CREATE TABLE IF NOT EXISTS agent_run_events_v3 (
    run_id TEXT NOT NULL,
    project_id TEXT NOT NULL,
    conversation_id TEXT NOT NULL,
    sequence INTEGER NOT NULL CHECK (sequence > 0),
    previous_hash TEXT NOT NULL,
    event_hash TEXT NOT NULL UNIQUE,
    value_json TEXT NOT NULL,
    PRIMARY KEY (run_id, sequence)
);

CREATE TABLE IF NOT EXISTS agent_run_snapshots_v3 (
    run_id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL,
    conversation_id TEXT NOT NULL,
    last_sequence INTEGER NOT NULL DEFAULT 0,
    last_event_hash TEXT NOT NULL DEFAULT '',
    value_json TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS agent_runs_v4 (
    run_id TEXT PRIMARY KEY CHECK (length(trim(run_id)) > 0),
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    conversation_id TEXT NOT NULL REFERENCES conversation_records(frame_id) ON DELETE CASCADE,
    status TEXT NOT NULL CHECK (length(trim(status)) > 0),
    value_json TEXT NOT NULL,
    created_at INTEGER NOT NULL DEFAULT 0,
    updated_at INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS agent_events_v4 (
    run_id TEXT NOT NULL REFERENCES agent_runs_v4(run_id) ON DELETE CASCADE,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    conversation_id TEXT NOT NULL REFERENCES conversation_records(frame_id) ON DELETE CASCADE,
    sequence INTEGER NOT NULL CHECK (sequence > 0),
    previous_hash TEXT NOT NULL,
    event_hash TEXT NOT NULL UNIQUE,
    value_json TEXT NOT NULL,
    occurred_at INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (run_id, sequence)
);

CREATE TABLE IF NOT EXISTS agent_context_archives_v4 (
    archive_id TEXT PRIMARY KEY CHECK (length(trim(archive_id)) > 0),
    run_id TEXT NOT NULL REFERENCES agent_runs_v4(run_id) ON DELETE CASCADE,
    through_sequence INTEGER NOT NULL CHECK (through_sequence >= 0),
    size_bytes INTEGER NOT NULL CHECK (size_bytes >= 0),
    sha256 TEXT NOT NULL,
    transcript_json TEXT NOT NULL,
    checkpoint_json TEXT NOT NULL,
    created_at INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS scientific_states_v4 (
    project_id TEXT PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
    revision INTEGER NOT NULL CHECK (revision >= 0),
    state_sha256 TEXT NOT NULL,
    value_json TEXT NOT NULL,
    updated_at INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS scientific_datasets_v4 (
    id TEXT PRIMARY KEY CHECK (length(trim(id)) > 0),
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    active INTEGER NOT NULL CHECK (active IN (0, 1)),
    value_json TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS scientific_analyses_v4 (
    id TEXT PRIMARY KEY CHECK (length(trim(id)) > 0),
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    status TEXT NOT NULL,
    value_json TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS scientific_artifacts_v4 (
    id TEXT PRIMARY KEY CHECK (length(trim(id)) > 0),
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    producer_analysis_id TEXT NOT NULL,
    valid INTEGER NOT NULL CHECK (valid IN (0, 1)),
    value_json TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS scientific_evidence_v4 (
    id TEXT PRIMARY KEY CHECK (length(trim(id)) > 0),
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    valid INTEGER NOT NULL CHECK (valid IN (0, 1)),
    value_json TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS scientific_provenance_v4 (
    id TEXT PRIMARY KEY CHECK (length(trim(id)) > 0),
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    run_id TEXT NOT NULL,
    analysis_id TEXT NOT NULL,
    complete INTEGER NOT NULL CHECK (complete IN (0, 1)),
    value_json TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_frames_project_updated
    ON frames(project_id, updated_at DESC, id);
CREATE INDEX IF NOT EXISTS idx_conversations_project_updated
    ON conversation_records(project_id, updated_at DESC, frame_id);
CREATE INDEX IF NOT EXISTS idx_messages_frame_seq
    ON messages(frame_id, seq, id);
CREATE INDEX IF NOT EXISTS idx_artifacts_project_created
    ON artifacts(project_id, created_at DESC, id);
CREATE INDEX IF NOT EXISTS idx_runs_project_updated
    ON runs(project_id, updated_at DESC, id);
CREATE INDEX IF NOT EXISTS idx_agent_runs_context
    ON agent_runs_v4(project_id, conversation_id, run_id);
CREATE INDEX IF NOT EXISTS idx_agent_events_context
    ON agent_events_v4(project_id, conversation_id, run_id, sequence);
CREATE INDEX IF NOT EXISTS idx_proposed_plans_context_revision
    ON proposed_plans(project_id, frame_id, revision DESC, id DESC);
CREATE INDEX IF NOT EXISTS idx_proposed_plans_active
    ON proposed_plans(project_id, frame_id, status, revision DESC);
CREATE INDEX IF NOT EXISTS idx_scientific_datasets_project
    ON scientific_datasets_v4(project_id, id);
CREATE INDEX IF NOT EXISTS idx_notebook_project_updated
    ON notebook_entries(project_id, updated_at DESC, id);
CREATE INDEX IF NOT EXISTS idx_sync_project
    ON sync_entries(project_id, id);
