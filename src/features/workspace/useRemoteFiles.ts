import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listRemoteFiles } from "../../tauri-api";
import type { RemoteFileEntry, WorkspaceProject } from "../../types";
import type { FileListingError, FileListingStatus } from "./RemoteFileTree";

type ProjectBinding = Pick<WorkspaceProject, "id" | "connection_id" | "remote_root">;
type ListingScope = { projectId: string; binding: string };
type ListingState = { scope: ListingScope | null; files: RemoteFileEntry[]; status: FileListingStatus; error?: FileListingError };

function listingErrorCode(error: unknown): FileListingError | undefined {
  const message = error instanceof Error ? error.message : typeof error === "string" ? error : "";
  const code = /^(remote_root_resolution_failed|remote_root_unavailable|remote_listing_failed)(?:$|[\s:])/.exec(message)?.[1];
  return code as FileListingError | undefined;
}

export function useRemoteFiles(project: ProjectBinding | null) {
  const projectId = project?.id;
  const connectionId = project?.connection_id;
  const remoteRoot = project?.remote_root;
  const scope = useMemo<ListingScope | null>(() => projectId && connectionId && remoteRoot
    ? { projectId, binding: JSON.stringify([connectionId, remoteRoot]) }
    : null, [projectId, connectionId, remoteRoot]);
  const currentScope = useRef(scope);
  currentScope.current = scope;
  const mounted = useRef(false);
  const requestVersion = useRef(0);
  const [listing, setListing] = useState<ListingState>({ scope: null, files: [], status: "idle" });

  const refresh = useCallback(async (requestedProjectId = scope?.projectId) => {
    if (!mounted.current || !scope || currentScope.current !== scope || requestedProjectId !== scope.projectId) return;
    const request = ++requestVersion.current;
    const isCurrent = () => mounted.current && currentScope.current === scope && requestVersion.current === request;
    setListing((previous) => ({ scope, files: previous.scope === scope ? previous.files : [], status: "loading" }));
    try {
      const files = await listRemoteFiles(scope.projectId);
      if (isCurrent()) setListing({ scope, files, status: "ready" });
    } catch (error) {
      // Native failures may contain host details; the file tree presents a safe retry message.
      if (isCurrent()) setListing((previous) => ({ scope, files: previous.scope === scope ? previous.files : [], status: "error", error: listingErrorCode(error) }));
    }
  }, [scope]);

  useEffect(() => {
    mounted.current = true;
    void refresh();
    return () => { mounted.current = false; requestVersion.current++; };
  }, [refresh]);

  const visible = listing.scope === scope ? listing : { files: [], status: "idle" as const, error: undefined };
  return { files: visible.files, status: visible.status, error: visible.error, refresh };
}
