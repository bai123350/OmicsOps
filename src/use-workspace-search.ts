import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { composerReferenceCatalog } from "./composer-reference-api";
import { buildWorkspaceSearchEntries, type WorkspaceSearchEntry } from "./workspace-search";

export type WorkspaceSearchProject = {
  id: string;
  name: string;
  description?: string;
};

export type WorkspaceSearchState = {
  entries: WorkspaceSearchEntry[];
  loading: boolean;
  failedProjects: number;
  retry: () => void;
};

type CatalogSnapshot = {
  signature: string;
  catalogs: ReadonlyMap<string, Awaited<ReturnType<typeof composerReferenceCatalog>>>;
};

const EMPTY_CATALOGS: ReadonlyMap<string, Awaited<ReturnType<typeof composerReferenceCatalog>>> = new Map();

function projectSignature(projects: WorkspaceSearchProject[]): string {
  return JSON.stringify(projects.map(({ id, name, description }) => [id, name, description ?? ""]));
}

/**
 * Build the shared workspace search index and lazily hydrate project-owned
 * references while the search surface is open.
 *
 * Project rows are available during the asynchronous catalog load. Each
 * request batch is identified by the current open state, project signature,
 * and retry generation so late results cannot overwrite a newer search.
 */
export function useWorkspaceSearch(
  open: boolean,
  projects: WorkspaceSearchProject[],
): WorkspaceSearchState {
  // Compute the primitive signature every render so even an in-place update
  // is observed, while the effect still ignores equivalent new array objects.
  const signature = projectSignature(projects);
  const projectIds = useMemo(() => projects.map(({ id }) => id), [signature]);
  const [snapshot, setSnapshot] = useState<CatalogSnapshot>({ signature: "", catalogs: EMPTY_CATALOGS });
  const [loadingState, setLoadingState] = useState(open);
  const [failedProjectsState, setFailedProjectsState] = useState(0);
  const [retryGeneration, setRetryGeneration] = useState(0);
  const requestGeneration = useRef(0);

  const retry = useCallback(() => {
    setRetryGeneration((generation) => generation + 1);
  }, []);

  useEffect(() => {
    const generation = requestGeneration.current + 1;
    requestGeneration.current = generation;
    let disposed = false;
    const current = () => !disposed && requestGeneration.current === generation;

    if (!open) {
      setSnapshot({ signature: "", catalogs: EMPTY_CATALOGS });
      setLoadingState(false);
      setFailedProjectsState(0);
      return () => {
        disposed = true;
      };
    }

    setSnapshot({ signature: "", catalogs: EMPTY_CATALOGS });
    setFailedProjectsState(0);
    if (projectIds.length === 0) {
      setLoadingState(false);
      setSnapshot({ signature, catalogs: EMPTY_CATALOGS });
      return () => {
        disposed = true;
      };
    }
    setLoadingState(true);

    void Promise.allSettled(projectIds.map((projectId) => composerReferenceCatalog(projectId))).then((results) => {
      if (!current()) return;

      const catalogs = new Map<string, Awaited<ReturnType<typeof composerReferenceCatalog>>>();
      let failedProjects = 0;
      results.forEach((result, index) => {
        if (result.status === "fulfilled") {
          catalogs.set(projectIds[index], result.value);
        } else {
          failedProjects += 1;
        }
      });
      setSnapshot({ signature, catalogs });
      setFailedProjectsState(failedProjects);
      setLoadingState(false);
    });

    return () => {
      disposed = true;
    };
  }, [open, projectIds, retryGeneration, signature]);

  const currentSnapshot = open && snapshot.signature === signature;
  const catalogs = currentSnapshot ? snapshot.catalogs : EMPTY_CATALOGS;
  const entries = useMemo(
    () => buildWorkspaceSearchEntries(projects, catalogs),
    [catalogs, projects],
  );

  return {
    entries,
    loading: open && projectIds.length > 0 && (loadingState || !currentSnapshot),
    failedProjects: currentSnapshot ? failedProjectsState : 0,
    retry,
  };
}
