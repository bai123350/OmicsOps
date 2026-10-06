import { useCallback, useSyncExternalStore, type Dispatch, type SetStateAction } from "react";
import { ComposerDraftStore, type DraftReceipt, type DraftScope } from "./composerDrafts";

export function useScopedComposerDraft(store: ComposerDraftStore, scope: DraftScope): {
  draft: string;
  setDraft: Dispatch<SetStateAction<string>>;
  capture: () => DraftReceipt;
} {
  const { projectId, conversationId } = scope;
  const subscribe = useCallback((listener: () => void) =>
    store.subscribe({ projectId, conversationId }, listener), [store, projectId, conversationId]);
  const read = useCallback(() => store.read({ projectId, conversationId }),
    [store, projectId, conversationId]);
  const snapshot = useSyncExternalStore(subscribe, read);
  const setDraft = useCallback<Dispatch<SetStateAction<string>>>((updater) => {
    store.update({ projectId, conversationId }, updater);
  }, [store, projectId, conversationId]);
  const capture = useCallback(() => store.capture({ projectId, conversationId }),
    [store, projectId, conversationId]);
  return { draft: snapshot.text, setDraft, capture };
}
