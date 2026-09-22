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
