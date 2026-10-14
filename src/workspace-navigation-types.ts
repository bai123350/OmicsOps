export type SourceKind = "message" | "tool" | "dataset" | "analysis" | "artifact" | "evidence" | "provenance" | "legacy_artifact" | "run";
export interface WorkspaceSourceRef {
  project_id: string; kind: SourceKind; id: string;
  conversation_id: string | null; run_id: string | null;
  sequence: number | null; event_hash: string | null;
  content_sha256: string | null; start: number | null; end: number | null;
}
export interface WorkspaceSourceSnapshot {
  source: WorkspaceSourceRef; title: string; text: string; sha256: string;
  status: string; metadata: Record<string, string>;
  availability: "available" | "changed" | "missing" | "unavailable";
}
export interface ConversationGroup { id: string; project_id: string; name: string; created_at: string; updated_at: string }
export interface ConversationGroupState { groups: ConversationGroup[]; memberships: { conversation_id: string; group_id: string }[] }
export interface JourneyEntry { source: WorkspaceSourceRef; title: string; status: string; occurred_at: string; summary: string }
export interface JourneyPage { entries: JourneyEntry[]; next_offset: number | null }
export interface PublicationSummary { id: string; project_id: string; title: string; revision: number; updated_at: string }
export interface PublicationRevision { id: string; publication_id: string; revision: number; title: string; markdown: string; references: WorkspaceSourceSnapshot[]; sha256: string; created_at: string; legacy: boolean }
export interface PublicationDetail { publication: PublicationSummary; revisions: PublicationRevision[] }
// Serde wire shapes from omicsops-dto/src/publication_evidence.rs.
export interface PublicationSelectionV1 { project_id: string; publication_id: string; revision: number; expected_revision_sha256: string }
export type PublicationRevisionHashScopeV1 = "stored_revision_content" | "public_legacy_markdown";
export type ReadinessFindingSeverityV1 = "attention" | "unverified" | "info";
export type ReadinessFindingCodeV1 = "source_changed" | "source_missing" | "source_unavailable" | "invalid_evidence" | "invalid_artifact" | "stale_analysis" | "incomplete_provenance" | "missing_random_seed" | "missing_software_version" | "missing_database_version" | "missing_sample" | "invalid_input" | "provenance_not_recorded" | "lineage_not_recorded" | "original_file_not_checked" | "historical_location_unverified" | "legacy_revision" | "inspection_limit";
export interface ReadinessFindingV1 { code: ReadinessFindingCodeV1; severity: ReadinessFindingSeverityV1; source: WorkspaceSourceRef | null; related_record_id: string | null; message: string }
export type ReproductionStatusV1 = "not_executed";
export interface PublicationReadinessReportV1 {
  schema_version: number; selection: PublicationSelectionV1; revision_hash_scope: PublicationRevisionHashScopeV1;
  observed_at: string; scientific_revision: number | null; scientific_state_sha256: string | null;
  references_checked: number; related_records_checked: number; attention_count: number; unverified_count: number;
  findings: ReadinessFindingV1[]; truncated: boolean; reproduction_status: ReproductionStatusV1;
}
export interface PublicationPackageExportResultV1 { path: string; package_sha256: string; size_bytes: number; manifest_sha256: string; selection: PublicationSelectionV1 }
export type PublicationEvidenceErrorCodeV1 = "invalid_selection" | "not_found" | "revision_changed" | "limit_exceeded" | "unsafe_public_content" | "storage_read_failed" | "export_write_failed";
export interface PublicationEvidenceErrorV1 { code: PublicationEvidenceErrorCodeV1; message: string }
// Serde wire shapes from omicsops-dto/src/workspace_files.rs. Unknowns stay null.
export interface ProjectFileSelectionV1 { backend_id: string; relative_path: string; expected_full_sha256: string | null }
export type WorkspaceContentModeV1 = "text_snapshot" | "reference_only";
export type FileReferenceHashScopeV1 = "full_file" | "recorded_file";
export interface FileReferenceHashV1 { sha256: string; scope: FileReferenceHashScopeV1 }
export interface CreateFileBaselineRequestV1 { request_id: string; project_id: string; branch_conversation_id: string | null; title: string; files: ProjectFileSelectionV1[] }
export interface ListFileBaselinesRequestV1 { project_id: string; branch_conversation_id: string | null; offset: number; limit: number }
export interface DiffFileBaselineRequestV1 { project_id: string; baseline_id: string; expected_manifest_sha256: string }
export interface ReadBaselineTextRequestV1 extends DiffFileBaselineRequestV1 { entry_index: number }
export interface BaselineTextSnapshotV1 { project_id: string; baseline_id: string; entry_index: number; relative_path: string; full_sha256: string; text: string }
export type BaselineOmissionCodeV1 = "remote_reference" | "too_large" | "unsupported_content" | "sensitive_content" | "missing" | "unreadable" | "unsafe_file" | "aggregate_budget";
export interface FileBaselineEntryV1 { backend_id: string; relative_path: string | null; content_mode: WorkspaceContentModeV1; size_bytes: number | null; hash: FileReferenceHashV1 | null; omission: BaselineOmissionCodeV1 | null; captured_at: string }
export interface FileBaselineV1 { schema_version: number; id: string; project_id: string; branch_conversation_id: string | null; title: string; entries: FileBaselineEntryV1[]; manifest_sha256: string; captured_text_bytes: number; created_at: string }
export interface FileBaselinePageV1 { items: FileBaselineV1[]; next_offset: number | null }
export type BaselineDiffStateV1 = "unchanged" | "changed" | "missing" | "unreadable" | "unsafe_file" | "reference_only" | "binding_changed" | "current_too_large" | "sensitive_current_content";
export type BaselineDiffLineKindV1 = "context" | "added" | "removed";
export interface BaselineDiffLineV1 { kind: BaselineDiffLineKindV1; text: string }
export interface BaselineTextHunkV1 { old_start: number; old_lines: number; new_start: number; new_lines: number; lines: BaselineDiffLineV1[] }
export interface FileBaselineDiffEntryV1 { baseline: FileBaselineEntryV1; state: BaselineDiffStateV1; current_size_bytes: number | null; current_hash: FileReferenceHashV1 | null; line_endings_changed: boolean | null; final_newline_changed: boolean | null; hunks: BaselineTextHunkV1[]; hunks_truncated: boolean }
export interface FileBaselineDiffV1 { project_id: string; baseline_id: string; manifest_sha256: string; observed_at: string; entries: FileBaselineDiffEntryV1[] }
export type FileBaselineErrorCodeV1 = "invalid_selection" | "not_found" | "ownership_mismatch" | "request_conflict" | "source_changed" | "manifest_changed" | "limit_exceeded" | "storage_failed";
export interface FileBaselineErrorV1 { code: FileBaselineErrorCodeV1; message: string }
export type LibraryKind = "code" | "excerpt" | "artifact";
export interface LibrarySummary { id: string; kind: LibraryKind; title: string; source_project_id: string; source_project_name: string; source_conversation_id: string | null; source_conversation_title: string | null; text_preview: string; created_at: string }
export interface LibraryDetail { item: LibrarySummary; snapshot: WorkspaceSourceSnapshot }
export interface LibrarySourceProject { id: string; name: string }
export interface LibraryPage { items: LibrarySummary[]; next_offset: number | null; source_projects?: LibrarySourceProject[] }
export interface SaveConversationGroupRequest { request_id: string; project_id: string; group_id: string | null; name: string }
export interface MoveConversationsRequest { project_id: string; group_id: string | null; conversation_ids: string[] }
export interface WorkspaceJourneyRequest { project_id: string; query: string; kind: SourceKind | null; status?: string | null; offset: number; limit: number }
export interface SaveWorkspacePublicationRequest { request_id: string; project_id: string; publication_id: string | null; expected_revision: number; title: string; markdown: string; sources: WorkspaceSourceRef[] }
export interface RestoreWorkspacePublicationRequest { request_id: string; project_id: string; publication_id: string; expected_revision: number; revision: number }
export interface WorkspaceLibraryRequest { query: string; kind: LibraryKind | null; project_id: string | null; offset: number; limit: number }
export interface SaveWorkspaceLibraryItemRequest { request_id: string; title: string; kind: LibraryKind; source: WorkspaceSourceRef }

export function workspaceSource(projectId: string, kind: SourceKind, id: string, fields: Partial<WorkspaceSourceRef> = {}): WorkspaceSourceRef {
  return { project_id: projectId, kind, id, conversation_id: null, run_id: null, sequence: null, event_hash: null, content_sha256: null, start: null, end: null, ...fields };
}
