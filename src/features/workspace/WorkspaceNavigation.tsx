import { useEffect, useRef, useState, type ReactNode } from "react";
import { ArrowLeft, BookOpen, ChevronDown, ChevronLeft, ChevronRight, File, FolderPlus, History, Pencil, Plus, Search, SlidersHorizontal, Star, Trash2, X } from "lucide-react";
import type { WorkspaceConversation } from "../../types";
import type { ConversationGroup, ConversationGroupState, SaveConversationGroupRequest } from "../../workspace-navigation-types";
import { deleteConversationGroup, listConversationGroups, moveConversations, saveConversationGroup } from "../../workspace-navigation-api";
import { useWindowEscapeLayer } from "../settings/BrowserSettings";
import type { Locale } from "./copy";
import "./workspace-navigation.css";

export type WorkspacePage = "conversation" | "files" | "journey" | "publication" | "library";
type DateBucket = "today" | "yesterday" | "week" | "earlier";
export function conversationDateBucket(value: string, now = new Date()): DateBucket {
  const date = new Date(value);
  if (!Number.isFinite(date.getTime())) return "earlier";
  const today = Date.UTC(now.getFullYear(), now.getMonth(), now.getDate());
  const day = Date.UTC(date.getFullYear(), date.getMonth(), date.getDate());
  const age = (today - day) / 86_400_000;
  return age <= 0 ? "today" : age === 1 ? "yesterday" : age < 7 ? "week" : "earlier";
}
interface Props {
  projectId: string; projectName: string; locale: Locale;
  conversations: WorkspaceConversation[]; activeConversationId?: string | null;
  collapsed: boolean; onToggleCollapsed: () => void; page?: WorkspacePage;
  onNavigate: (page: WorkspacePage) => void;
  onNewConversation?: () => void | Promise<void>; newConversationDisabled?: boolean;
  onSelectConversation?: (id: string) => void | Promise<void>;
  onDeleteConversation?: (conversation: WorkspaceConversation) => void | Promise<void>;
  deleteDisabled?: (conversation: WorkspaceConversation) => boolean;
  onOpenSearch?: () => void; onBack?: () => void; children?: ReactNode;
}

export function WorkspaceNavigation(props: Props) {
  const { projectId, projectName, locale, conversations, collapsed } = props;
  const zh = locale === "zh-CN";
  const [groups, setGroups] = useState<ConversationGroupState>({ groups: [], memberships: [] });
  const [loading, setLoading] = useState(true);
  const [refresh, setRefresh] = useState(0);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [filters, setFilters] = useState(false);
  const [query, setQuery] = useState("");
  const [status, setStatus] = useState("");
  const [groupFilter, setGroupFilter] = useState("");
  const [sort, setSort] = useState("updated");
  const [selecting, setSelecting] = useState(false);
  const [selected, setSelected] = useState<string[]>([]);
  const [targetGroup, setTargetGroup] = useState("");
  const [expanded, setExpanded] = useState<string[]>([]);
  const [editor, setEditor] = useState<{ id: string | null; name: string } | null>(null);
  const [editorError, setEditorError] = useState("");
  const pendingSave = useRef<SaveConversationGroupRequest | null>(null);
  const mounted = useRef(true);
  const operation = useRef(false);
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; }; }, []);
  const conversationIds = conversations.map((item) => item.id).sort().join(",");
  useEffect(() => {
    let alive = true;
    setLoading(true);
    listConversationGroups(projectId).then((value) => { if (alive) { setGroups(value); setError(""); } }, () => { if (alive) setError(zh ? "无法加载会话分组。" : "Could not load session groups."); }).finally(() => { if (alive) setLoading(false); });
    return () => { alive = false; };
  }, [projectId, refresh, conversationIds, zh]);
  useEffect(() => { setSelected((ids) => ids.filter((id) => conversations.some((item) => item.id === id))); }, [conversationIds]);
  useWindowEscapeLayer(filters && !collapsed, () => setFilters(false));
  useWindowEscapeLayer(editor !== null, () => { if (!busy) setEditor(null); });

  function editGroup(group?: ConversationGroup) {
    setFilters(false);
    const pending = pendingSave.current;
    setEditor(pending ? { id: pending.group_id, name: pending.name } : { id: group?.id ?? null, name: group?.name ?? "" });
    setEditorError("");
  }
  async function saveGroup() {
    if (!editor || operation.current || !editor.name.trim()) return;
    operation.current = true; setBusy(true); setEditorError("");
    const request = pendingSave.current ?? { request_id: crypto.randomUUID(), project_id: projectId, group_id: editor.id, name: editor.name.trim() };
    pendingSave.current = request;
    try {
      await saveConversationGroup(request);
      if (mounted.current) { pendingSave.current = null; setEditor(null); setRefresh((n) => n + 1); }
    } catch (cause) {
      if (mounted.current) {
        const message = cause instanceof Error ? cause.message : String(cause);
        // Only these explicit host rejections prove no group write committed.
        // Unknown errors can include a lost reply after commit and must replay.
        const invalidName = /^invalid input: group name (?:must contain 1\.\.80 characters|exceeds \d+ bytes)$/.test(message);
        const duplicateName = /^database failed: .*UNIQUE constraint failed: conversation_groups\.project_id, conversation_groups\.name$/.test(message);
        if (invalidName || duplicateName) {
          pendingSave.current = null;
          setEditorError(duplicateName ? (zh ? "此分组名称已存在，请使用其他名称。" : "This group name already exists. Choose another name.") : (zh ? "分组名称须为 1–80 个字符，请修改后重试。" : "Group names must contain 1–80 characters. Edit the name and retry."));
        } else setEditorError(zh ? "保存失败。可重试同一次保存。" : "Save failed. Retry the same save.");
      }
    }
    finally { operation.current = false; if (mounted.current) setBusy(false); }
  }
  async function mutate(action: () => Promise<void>) {
    if (operation.current) return;
    operation.current = true; setBusy(true); setError("");
    try { await action(); if (mounted.current) { setSelected([]); setRefresh((n) => n + 1); } }
    catch { if (mounted.current) setError(zh ? "操作失败，请重试。" : "Action failed. Please retry."); }
    finally { operation.current = false; if (mounted.current) setBusy(false); }
  }

  const groupByConversation = new Map(groups.memberships.map((item) => [item.conversation_id, item.group_id]));
  const filtered = conversations.filter((item) => item.title.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase()) && (!status || item.status === status) && (!groupFilter || (groupFilter === "ungrouped" ? !groupByConversation.has(item.id) : groupByConversation.get(item.id) === groupFilter))).sort((a, b) => {
    const time = (value: string) => Number.isFinite(Date.parse(value)) ? Date.parse(value) : 0;
    return sort === "created" ? time(a.created_at) - time(b.created_at) : time(b.updated_at) - time(a.updated_at);
  });
  const labels: Record<DateBucket, string> = zh ? { today: "今天", yesterday: "昨天", week: "近 7 天", earlier: "更早" } : { today: "Today", yesterday: "Yesterday", week: "Previous 7 days", earlier: "Earlier" };
  function rows(items: WorkspaceConversation[]) {
    return items.map((item) => {
      const title = item.title || (zh ? "新会话" : "New conversation");
      return <div className={`wn-session ${item.id === props.activeConversationId ? "active" : ""}`} key={item.id}>
        {selecting && <input type="checkbox" aria-label={`${zh ? "选择" : "Select"}: ${title}`} checked={selected.includes(item.id)} onChange={(event) => setSelected((ids) => event.target.checked ? [...ids, item.id] : ids.filter((id) => id !== item.id))} />}
        <button className="wn-session-link" aria-current={item.id === props.activeConversationId ? "page" : undefined} title={title} onClick={() => void props.onSelectConversation?.(item.id)}>{title}</button>
        {!selecting && props.onDeleteConversation && <button className="wn-icon wn-delete" aria-label={zh ? `删除会话：${title}` : `Delete conversation: ${title}`} disabled={props.deleteDisabled?.(item)} onClick={() => void props.onDeleteConversation?.(item)}><Trash2 size={14} /></button>}
      </div>;
    });
  }
  const destinations = [
    { id: "files" as const, label: zh ? "文件" : "Files", icon: File },
    { id: "journey" as const, label: zh ? "研究历程" : "Research journey", icon: History },
    { id: "publication" as const, label: zh ? "发表工作区" : "Publication", icon: BookOpen },
    { id: "library" as const, label: zh ? "资料库" : "Library", icon: Star },
  ];
  return <nav className={`project-rail workspace-navigation ${collapsed ? "wn-collapsed" : ""}`} aria-label={zh ? "项目与会话" : "Projects and sessions"}>
    <header className="wn-header">
      {!collapsed && <><button className="wn-icon" aria-label={zh ? "返回项目主页" : "Back to project home"} onClick={props.onBack}><ArrowLeft size={19} /></button><div><strong>{zh ? "工作区" : "Workspace"}</strong><small title={projectName}>{projectName}</small></div></>}
      <button className="wn-icon" aria-label={collapsed ? (zh ? "展开工作区导航" : "Expand workspace navigation") : (zh ? "折叠工作区导航" : "Collapse workspace navigation")} onClick={props.onToggleCollapsed}>{collapsed ? <ChevronRight size={18} /> : <ChevronLeft size={18} />}</button>
    </header>
    <div className="wn-actions">
      <button aria-label={zh ? "新建会话" : "New conversation"} title={zh ? "新建会话 · Ctrl+N" : "New conversation · Ctrl+N"} disabled={props.newConversationDisabled || !props.onNewConversation} onClick={() => void props.onNewConversation?.()}><Plus size={19} /><span>{zh ? "新建会话" : "New session"}</span><kbd>Ctrl+N</kbd></button>
      <button aria-label={zh ? "搜索项目" : "Search projects"} title={zh ? "搜索 · Ctrl+K" : "Search · Ctrl+K"} onClick={props.onOpenSearch} disabled={!props.onOpenSearch}><Search size={19} /><span>{zh ? "搜索" : "Search"}</span><kbd>Ctrl+K</kbd></button>
      <button aria-label={zh ? "新建分组" : "New group"} title={zh ? "新建分组" : "New group"} onClick={() => editGroup()} disabled={busy}><FolderPlus size={19} /><span>{zh ? "新建分组" : "New group"}</span></button>
      {destinations.map(({ id, label, icon: Icon }) => <button key={id} aria-label={label} title={label} aria-current={props.page === id ? "page" : undefined} onClick={() => props.onNavigate(id)}><Icon size={19} /><span>{label}</span></button>)}
    </div>
    {!collapsed && <section className="wn-sessions" aria-label={zh ? "会话列表" : "Sessions"}>
      <header><b>{zh ? "会话" : "Sessions"}</b><button aria-label={zh ? "选择会话" : "Select sessions"} aria-pressed={selecting} onClick={() => { setSelecting(!selecting); setSelected([]); }}>{selecting ? (zh ? "完成" : "Done") : (zh ? "选择" : "Select")}</button><button className="wn-icon" aria-label={zh ? "筛选会话" : "Filter sessions"} aria-expanded={filters} onClick={() => setFilters(!filters)}><SlidersHorizontal size={17} /></button></header>
      {filters && <div className="wn-filters">
        <input aria-label={zh ? "筛选会话标题" : "Filter session titles"} placeholder={zh ? "搜索会话…" : "Filter sessions…"} value={query} onChange={(event) => setQuery(event.target.value)} />
        <select aria-label={zh ? "会话状态" : "Session status"} value={status} onChange={(event) => setStatus(event.target.value)}><option value="">{zh ? "全部状态" : "All statuses"}</option>{["idle", "running", "waiting_for_input", "needs_attention", "completed", "archived"].map((value) => <option key={value} value={value}>{value}</option>)}</select>
        <select aria-label={zh ? "会话分组" : "Session group"} value={groupFilter} onChange={(event) => setGroupFilter(event.target.value)}><option value="">{zh ? "全部分组" : "All groups"}</option><option value="ungrouped">{zh ? "未分组" : "Ungrouped"}</option>{groups.groups.map((group) => <option key={group.id} value={group.id}>{group.name}</option>)}</select>
        <select aria-label={zh ? "会话排序" : "Session order"} value={sort} onChange={(event) => setSort(event.target.value)}><option value="updated">{zh ? "最近更新" : "Recently updated"}</option><option value="created">{zh ? "最早创建" : "Oldest created"}</option></select>
      </div>}
      {selecting && <div className="wn-selection"><small>{selected.length} {zh ? "已选" : "selected"}</small><select aria-label={zh ? "移动所选会话到分组" : "Move selected to group"} value={targetGroup} onChange={(event) => setTargetGroup(event.target.value)}><option value="">{zh ? "移出分组" : "Ungrouped"}</option>{groups.groups.map((group) => <option key={group.id} value={group.id}>{group.name}</option>)}</select><button disabled={!selected.length || busy} onClick={() => void mutate(() => moveConversations({ project_id: projectId, group_id: targetGroup || null, conversation_ids: selected }))}>{zh ? "移动" : "Move"}</button></div>}
      {error && <p role="alert">{error}<button onClick={() => setRefresh((n) => n + 1)}>{zh ? "重试" : "Retry"}</button></p>}
      <div className="wn-session-scroll" aria-busy={loading}>
        {sort !== "created" && groups.groups.filter((group) => !groupFilter || groupFilter === group.id).map((group) => {
          const items = filtered.filter((item) => groupByConversation.get(item.id) === group.id);
          const open = expanded.includes(group.id) || !!query || !!status || groupFilter === group.id;
          return <section className="wn-group" key={group.id}><header><button aria-label={`${open ? (zh ? "收起分组" : "Collapse group") : (zh ? "展开分组" : "Expand group")}: ${group.name}`} aria-expanded={open} onClick={() => setExpanded((ids) => ids.includes(group.id) ? ids.filter((id) => id !== group.id) : [...ids, group.id])}>{open ? <ChevronDown size={14} /> : <ChevronRight size={14} />}<span>{group.name}</span><small>{items.length}</small></button><button className="wn-icon" aria-label={`${zh ? "重命名分组" : "Rename group"}: ${group.name}`} disabled={busy} onClick={() => editGroup(group)}><Pencil size={12} /></button><button className="wn-icon" aria-label={`${zh ? "删除分组（保留会话）" : "Delete group (keep sessions)"}: ${group.name}`} disabled={busy} onClick={() => void mutate(() => deleteConversationGroup(projectId, group.id))}><X size={13} /></button></header>{open && rows(items)}</section>;
        })}
        {sort === "created" ? <section className="wn-date-group"><h3>{zh ? "最早创建" : "Oldest created"}</h3>{rows(filtered)}</section> : (Object.keys(labels) as DateBucket[]).map((bucket) => { const items = filtered.filter((item) => !groupByConversation.has(item.id) && conversationDateBucket(item.updated_at) === bucket); return items.length ? <section className="wn-date-group" key={bucket}><h3>{labels[bucket]}</h3>{rows(items)}</section> : null; })}
        {!filtered.length && <div className="wn-empty"><p>{zh ? "没有匹配的会话" : "No matching sessions"}</p>{(query || status || groupFilter) && <button onClick={() => { setQuery(""); setStatus(""); setGroupFilter(""); }}>{zh ? "清除筛选" : "Clear filters"}</button>}</div>}
      </div>
    </section>}
    <div className="wn-footer">{props.children}</div>
    {editor && <div className="wn-backdrop"><section className="wn-dialog" role="dialog" aria-modal="true" aria-labelledby="wn-group-title"><header><h2 id="wn-group-title">{editor.id ? (zh ? "重命名分组" : "Rename group") : (zh ? "新建分组" : "New group")}</h2><button className="wn-icon" disabled={busy} aria-label={zh ? "关闭分组编辑" : "Close group editor"} onClick={() => setEditor(null)}><X size={19} /></button></header><label>{zh ? "分组名称" : "Group name"}<input autoFocus maxLength={80} disabled={busy || !!pendingSave.current} value={editor.name} onChange={(event) => setEditor({ ...editor, name: event.target.value })} /></label>{editorError && <p role="alert">{editorError}</p>}<footer><button disabled={busy} onClick={() => setEditor(null)}>{zh ? "取消" : "Cancel"}</button><button disabled={busy || !editor.name.trim()} onClick={() => void saveGroup()}>{zh ? "保存分组" : "Save group"}</button></footer></section></div>}
  </nav>;
}
