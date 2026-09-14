import type { ComposerCatalogItem } from "./types";

export type WorkspaceSearchEntry = {
  key: string;
  kind: "project" | "session" | "artifact" | "skill" | "action";
  label: string;
  description: string;
  projectId?: string;
  item?: ComposerCatalogItem;
};

export type WorkspaceSearchRequest =
  | { key: string; kind: "reveal"; projectId: string }
  | { key: string; kind: "attach"; projectId: string; conversationId: string; item: ComposerCatalogItem }
  | { key: string; kind: "artifact"; projectId: string; item: ComposerCatalogItem }
  | { key: string; kind: "files"; projectId: string };

export function buildWorkspaceSearchEntries(
  projects: Array<{ id: string; name: string; description?: string }>,
  catalogs: ReadonlyMap<string, ComposerCatalogItem[]>,
): WorkspaceSearchEntry[] {
  const entries: WorkspaceSearchEntry[] = [];
  const seen = new Set<string>();
  for (const project of projects) {
    const projectItem = catalogs.get(project.id)?.find((item) => item.reference.kind === "project" && item.reference.project_id === project.id && item.reference.id === project.id) ?? {
      reference: { kind: "project" as const, project_id: project.id, id: project.id },
      label: project.name,
      description: project.description ?? "",
    };
    entries.push({ key: `project:${project.id}`, kind: "project", projectId: project.id, label: projectItem.label, description: projectItem.description, item: projectItem });
    for (const item of catalogs.get(project.id) ?? []) {
      const reference = item.reference;
      if (reference.kind !== "artifact" && reference.kind !== "session" && reference.kind !== "skill") continue;
      if (reference.kind !== "skill" && reference.project_id !== project.id) continue;
      const key = reference.kind === "skill" ? `skill:${reference.id}` : `${reference.kind}:${reference.project_id}:${reference.id}`;
      if (seen.has(key)) continue;
      seen.add(key);
      entries.push({ key, kind: reference.kind, label: item.label, description: reference.kind === "skill" ? item.description : `${project.name} · ${item.description}`, projectId: reference.kind === "skill" ? undefined : project.id, item });
    }
  }
  return entries;
}

export function filterWorkspaceSearchEntries(entries: WorkspaceSearchEntry[], query: string): WorkspaceSearchEntry[] {
  const terms = query.trim().toLocaleLowerCase().split(/\s+/u).filter(Boolean);
  return entries.filter((entry) => {
    const text = `${entry.label}\n${entry.description}`.toLocaleLowerCase();
    return terms.every((term) => text.includes(term));
  });
}

export function canAttachSearchEntry(entry: WorkspaceSearchEntry, projectId?: string, conversationId?: string): boolean {
  const reference = entry.item?.reference;
  if (!reference || !projectId || !conversationId) return false;
  if (reference.kind === "skill") return true;
  if (reference.kind === "project") return reference.project_id === projectId && reference.id === projectId;
  if (reference.kind !== "artifact" && reference.kind !== "session") return false;
  return reference.project_id === projectId && !(reference.kind === "session" && reference.id === conversationId);
}
