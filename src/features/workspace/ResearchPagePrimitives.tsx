import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { X } from "lucide-react";
import { getWorkspaceSourceDetail, saveWorkspaceLibraryItem } from "../../workspace-navigation-api";
import type { WorkspaceSourceRef, WorkspaceSourceSnapshot } from "../../workspace-navigation-types";
import { useWindowEscapeLayer } from "../settings/BrowserSettings";

export function sourceKey(source: WorkspaceSourceRef): string {
  return JSON.stringify([source.project_id, source.kind, source.id, source.conversation_id, source.run_id, source.sequence, source.event_hash, source.content_sha256, source.start, source.end]);
}
// Keep uncertain host writes replayable when their detail dialog is closed.
const pendingCollections = new Map<string, Parameters<typeof saveWorkspaceLibraryItem>[0]>();
export function errorText(error: unknown): string { return error instanceof Error ? error.message : String(error); }
export function useAlive() {
  const alive = useRef(true);
  useEffect(() => { alive.current = true; return () => { alive.current = false; }; }, []);
  return alive;
}

/** Changing a query invalidates both initial and pagination responses. */
export function useResearchList<T, M = undefined>(key: string, fetchPage: (offset: number) => Promise<{ items: T[]; next: number | null; meta?: M }>) {
  const fetchRef = useRef(fetchPage);
  fetchRef.current = fetchPage;
  const generation = useRef(0);
  const pending = useRef(false);
  const [items, setItems] = useState<T[]>([]);
  const [meta, setMeta] = useState<M | undefined>();
  const [next, setNext] = useState<number | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const failedOffset = useRef(0);
  const load = useCallback(async (offset: number, reset = false) => {
    if (pending.current && !reset) return;
    const ticket = reset ? ++generation.current : generation.current;
    pending.current = true;
    setLoading(true); setError("");
    failedOffset.current = offset;
    if (reset) { setItems([]); setNext(null); }
    try {
      const page = await fetchRef.current(offset);
      if (ticket !== generation.current) return;
      setItems((previous) => offset === 0 ? page.items : [...previous, ...page.items]);
      setNext(page.next);
      setMeta(page.meta);
    } catch (cause) {
      if (ticket === generation.current) setError(errorText(cause));
    } finally {
      if (ticket === generation.current) { pending.current = false; setLoading(false); }
    }
  }, []);
  useEffect(() => { void load(0, true); return () => { generation.current += 1; pending.current = false; }; }, [key, load]);
  return { items, meta, next, loading, error, reload: () => void load(0, true), retry: () => void load(failedOffset.current), more: () => { if (next !== null) void load(next); } };
}

export function ResearchDialog({ title, onClose, children, zh, navigationDecision = false }: { title: string; onClose: () => void; children: ReactNode; zh: boolean; navigationDecision?: boolean }) {
  useWindowEscapeLayer(true, onClose);
  return <div className="research-dialog-backdrop" data-navigation-decision={navigationDecision || undefined}><section className="research-dialog" role="dialog" aria-modal="true" aria-label={title}>
    <header><h2>{title}</h2><button type="button" aria-label={zh ? "关闭" : "Close"} onClick={onClose}><X size={18} /></button></header>
    <div className="research-dialog-content">{children}</div>
  </section></div>;
}

export function ResearchError({ error, retry, zh }: { error: string; retry?: () => void; zh: boolean }) {
  return error ? <div className="research-error" role="alert"><span>{error}</span>{retry && <button type="button" onClick={retry}>{zh ? "重试" : "Retry"}</button>}</div> : null;
}

export function SnapshotView({ snapshot, zh }: { snapshot: WorkspaceSourceSnapshot; zh: boolean }) {
  const artifact = snapshot.source.kind === "artifact" || snapshot.source.kind === "legacy_artifact";
  const availability = { available: zh ? "来源记录可用" : "Source record available", changed: zh ? "来源已变化" : "Source changed", missing: zh ? "来源已缺失" : "Source missing", unavailable: zh ? "来源暂不可用" : "Source unavailable" }[snapshot.availability];
  return <div className="research-snapshot">
    <h3>{snapshot.title}</h3>
    <div className="research-badges"><span>{snapshot.source.kind}</span><span>{snapshot.status}</span><span>{availability}</span></div>
    {artifact && <p className="research-note">{zh ? "产物引用：仅保存来源元数据，不包含文件内容；来源记录可用不表示文件仍存在。" : "Artifact reference: source metadata only. File contents are not included; an available record does not prove that the file still exists."}</p>}
    {snapshot.availability !== "available" && <p className="research-note">{zh ? "以下为保存时的快照，保留原摘要，不代表当前来源内容。" : "This is the saved snapshot and its original hash, which may differ from the current source."}</p>}
    <pre className="research-snapshot-text">{snapshot.text || (zh ? "没有文本内容；请查看来源元数据。" : "No text content; see source metadata.")}</pre>
    <dl className="research-metadata">
      <dt>{zh ? "快照 SHA-256" : "Snapshot SHA-256"}</dt><dd>{snapshot.sha256}</dd>
      <dt>{zh ? "来源 ID" : "Source ID"}</dt><dd>{snapshot.source.id}</dd>
      {snapshot.source.run_id && <><dt>{zh ? "运行" : "Run"}</dt><dd>{snapshot.source.run_id}</dd></>}
      {Object.entries(snapshot.metadata).map(([key, value]) => <div className="research-metadata-row" key={key}><dt>{key}</dt><dd>{value}</dd></div>)}
    </dl>
  </div>;
}

export function SourceDialog({ source, zh, onClose, onOpenConversation }: { source: WorkspaceSourceRef; zh: boolean; onClose: () => void; onOpenConversation: (projectId: string, conversationId: string) => void }) {
  const [snapshot, setSnapshot] = useState<WorkspaceSourceSnapshot | null>(null);
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);
  const [collecting, setCollecting] = useState(false);
  const [collected, setCollected] = useState(false);
  const [collectionError, setCollectionError] = useState("");
  const request = useRef<Parameters<typeof saveWorkspaceLibraryItem>[0] | null>(pendingCollections.get(sourceKey(source)) ?? null);
  const alive = useAlive();
  const load = useCallback(async () => {
    setError(""); setLoading(true);
    try { const value = await getWorkspaceSourceDetail(source); if (alive.current) setSnapshot(value); }
    catch (cause) { if (alive.current) setError(errorText(cause)); }
    finally { if (alive.current) setLoading(false); }
  }, [source, alive]);
  useEffect(() => { void load(); }, [load]);
  async function collect() {
    if ((!snapshot && !request.current) || collecting || collected) return;
    if (!request.current && snapshot) request.current = { request_id: crypto.randomUUID(), title: snapshot.title, kind: source.kind === "artifact" || source.kind === "legacy_artifact" ? "artifact" : "excerpt", source };
    if (!request.current) return;
    pendingCollections.set(sourceKey(source), request.current);
    setCollecting(true); setCollectionError("");
    try { await saveWorkspaceLibraryItem(request.current); pendingCollections.delete(sourceKey(source)); if (alive.current) setCollected(true); }
    catch (cause) { if (alive.current) setCollectionError(errorText(cause)); }
    finally { if (alive.current) setCollecting(false); }
  }
  return <ResearchDialog title={zh ? "来源详情" : "Source details"} onClose={onClose} zh={zh}>
    {loading && <p role="status">{zh ? "正在加载…" : "Loading…"}</p>}
    <ResearchError error={error} retry={() => void load()} zh={zh} />
    {snapshot && <><SnapshotView snapshot={snapshot} zh={zh} /><div className="research-actions">
      <button type="button" disabled={!source.conversation_id || snapshot.availability === "missing" || snapshot.availability === "unavailable"} onClick={() => { if (source.conversation_id) onOpenConversation(source.project_id, source.conversation_id); }}>{zh ? "打开来源会话" : "Open source conversation"}</button>
    </div>{!source.conversation_id && <p className="research-note">{zh ? "来源会话未记录。" : "Source conversation was not recorded."}</p>}</>}
    <ResearchError error={collectionError} zh={zh} />
    {(snapshot || request.current) && <button type="button" disabled={collecting || collected || (snapshot?.availability !== "available" && !request.current)} onClick={() => void collect()}>{collected ? (zh ? "已收藏" : "Added to library") : request.current ? (zh ? "重试收藏" : "Retry collection") : (zh ? "收藏到资料库" : "Add to library")}</button>}
    {collected && <p role="status">{zh ? "已收藏到资料库。" : "Added to library"}</p>}
  </ResearchDialog>;
}
