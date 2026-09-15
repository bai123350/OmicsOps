import { useCallback, useEffect, useRef, useState } from "react";
import { KeyRound, LoaderCircle, Plus, RefreshCw, ShieldCheck, Trash2 } from "lucide-react";

import {
  createCredential,
  deleteCredential,
  listCredentials,
  replaceCredential,
} from "../../credentials-settings-api";
import type { CredentialConsumer, CredentialEntry } from "../../types";
import type { Locale } from "../workspace/copy";
import { useWindowEscapeLayer } from "./BrowserSettings";
import "./CredentialsSettings.css";

type OwnerPage = "models" | "remote" | "connections";
type Editor = { entry: CredentialEntry; secret: string; path: string; passphrase: string };

interface Props {
  locale: Locale;
  onOpenOwner: (page: OwnerPage) => void;
}

export function CredentialsSettings({ locale, onOpenOwner }: Props) {
  const zh = locale === "zh-CN";
  const [entries, setEntries] = useState<CredentialEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [creating, setCreating] = useState(false);
  const [createDraft, setCreateDraft] = useState({ label: "", secret: "" });
  const [editor, setEditor] = useState<Editor | null>(null);
  const [confirmDelete, setConfirmDelete] = useState<CredentialEntry | null>(null);
  const generation = useRef(0);
  const editorOpen = creating || editor !== null || confirmDelete !== null;

  const closeCreate = useCallback(() => {
    if (busy) return;
    setCreating(false);
    setCreateDraft({ label: "", secret: "" });
  }, [busy]);
  const closeEditor = useCallback(() => {
    if (busy) return;
    setEditor(null);
  }, [busy]);
  const closeDelete = useCallback(() => {
    if (busy) return;
    setConfirmDelete(null);
  }, [busy]);
  useWindowEscapeLayer(creating, closeCreate);
  useWindowEscapeLayer(editor !== null, closeEditor);
  useWindowEscapeLayer(confirmDelete !== null, closeDelete);

  const load = useCallback(async () => {
    const request = ++generation.current;
    setLoading(true);
    setError("");
    try {
      const result = await listCredentials();
      if (request === generation.current) setEntries(result);
    } catch {
      if (request === generation.current) setError(zh ? "无法读取凭据目录，请重试。" : "Could not load the credential directory. Retry.");
    } finally {
      if (request === generation.current) setLoading(false);
    }
  }, [zh]);

  useEffect(() => {
    void load();
    return () => {
      ++generation.current;
      setCreateDraft({ label: "", secret: "" });
      setEditor(null);
    };
  }, [load]);

  async function submitCreate() {
    if (busy || !createDraft.label.trim() || !createDraft.secret.trim()) return;
    setBusy(true);
    setError("");
    try {
      const result = await createCredential(createDraft);
      setEntries((current) => upsert(current, result.entry));
      setCreating(false);
      if (result.kind === "saved") {
        setCreateDraft({ label: "", secret: "" });
      } else {
        setEditor({ entry: result.entry, secret: createDraft.secret, path: "", passphrase: "" });
        setCreateDraft({ label: "", secret: "" });
        setError(zh ? "系统 keyring 未能确认本次保存。请在此条目上重试替换，或取消后删除它。" : "The system keyring could not confirm this save. Retry replacement on this entry, or cancel and delete it.");
      }
    } catch {
      setError(zh ? "无法创建凭据目录条目。请核对输入后重试。" : "Could not create the credential directory entry. Check the input and retry.");
    } finally {
      setBusy(false);
    }
  }

  async function submitReplacement() {
    if (!editor || busy) return;
    const secret = editor.entry.value_kind === "ssh_private_key"
      ? JSON.stringify({ path: editor.path, passphrase: editor.passphrase || null })
      : editor.secret;
    if (!secret.trim() || (editor.entry.value_kind === "ssh_private_key" && !editor.path.trim())) return;
    setBusy(true);
    setError("");
    try {
      const updated = await replaceCredential({
        target: editor.entry.target,
        expected_reference: editor.entry.reference,
        expected_value_kind: editor.entry.value_kind,
        secret,
      });
      setEntries((current) => upsert(current, updated));
      setEditor(null);
    } catch {
      setError(zh ? "无法替换凭据。请检查新值；若绑定已变化，请刷新后重试。" : "Could not replace the credential. Check the new value; if its binding changed, refresh and retry.");
    } finally {
      setBusy(false);
    }
  }

  async function confirmDeletion() {
    if (!confirmDelete || confirmDelete.target.kind !== "managed" || busy) return;
    setBusy(true);
    setError("");
    try {
      const result = await deleteCredential(confirmDelete.target.id);
      if (result.kind === "deleted") {
        setEntries((current) => current.filter((entry) => entry.reference !== confirmDelete.reference));
        setConfirmDelete(null);
      } else {
        setEntries((current) => current.map((entry) => entry.reference === confirmDelete.reference
          ? { ...entry, consumers: result.consumers, can_delete: false }
          : entry));
        setConfirmDelete(null);
        setError(zh ? "此凭据已被新的配置引用，未执行删除。" : "A configuration now uses this credential, so it was not deleted.");
      }
    } catch {
      setError(zh ? "删除未完成。请刷新并核对当前状态后重试。" : "Deletion did not complete. Refresh to check the current state before retrying.");
    } finally {
      setBusy(false);
    }
  }

  return <main className="credentials-settings">
    <div className="settings-heading">
      <h3>{zh ? "凭据" : "Credentials"}</h3>
      <p>{zh ? "查看实际配置引用的凭据状态，并通过 Windows Credential Manager 或系统 keyring 替换秘密。UI 不会接收或显示现有秘密。" : "Review credentials referenced by actual configurations and replace secrets through Windows Credential Manager or the system keyring. The UI never receives or displays existing secrets."}</p>
    </div>
    <div className="credentials-toolbar">
      <button type="button" disabled={busy || loading || editorOpen} onClick={() => void load()}><RefreshCw size={14} />{zh ? "刷新" : "Refresh"}</button>
      <button type="button" className="primary" disabled={busy || loading} onClick={() => { setCreating(true); setEditor(null); setConfirmDelete(null); }}><Plus size={14} />{zh ? "新建凭据" : "New credential"}</button>
    </div>
    {error && <p className="credentials-error" role="alert">{error}</p>}
    {creating && <section className="credential-editor" role="dialog" aria-label={zh ? "新建凭据" : "New credential"}>
      <h4>{zh ? "新建受管凭据" : "New managed credential"}</h4>
      <label>{zh ? "名称" : "Label"}<input disabled={busy} aria-label={zh ? "凭据名称" : "Credential label"} maxLength={100} value={createDraft.label} onChange={(event) => setCreateDraft({ ...createDraft, label: event.target.value })} /></label>
      <label>{zh ? "秘密值" : "Secret value"}<input disabled={busy} aria-label={zh ? "秘密值" : "Secret value"} type="password" autoComplete="new-password" value={createDraft.secret} onChange={(event) => setCreateDraft({ ...createDraft, secret: event.target.value })} /></label>
      <div><button disabled={busy} onClick={closeCreate}>{zh ? "取消" : "Cancel"}</button><button className="primary" disabled={busy || !createDraft.label.trim() || !createDraft.secret.trim()} onClick={() => void submitCreate()}>{busy && <LoaderCircle className="spin" size={13} />}{zh ? "保存" : "Save"}</button></div>
    </section>}
    {editor && <section className="credential-editor" role="dialog" aria-label={zh ? `替换 ${editor.entry.label}` : `Replace ${editor.entry.label}`}>
      <h4>{zh ? `替换：${editor.entry.label}` : `Replace: ${editor.entry.label}`}</h4>
      <p>{zh ? "输入完整的新值。旧值不会被读取或回显。" : "Enter the complete new value. The old value is never read or shown."}</p>
      {editor.entry.value_kind === "ssh_private_key" ? <>
        <label>{zh ? "新私钥路径" : "New private-key path"}<input disabled={busy} aria-label={zh ? "新私钥路径" : "New private-key path"} value={editor.path} onChange={(event) => setEditor({ ...editor, path: event.target.value })} /></label>
        <label>{zh ? "新私钥口令（可选）" : "New passphrase (optional)"}<input disabled={busy} aria-label={zh ? "新私钥口令" : "New passphrase"} type="password" autoComplete="new-password" value={editor.passphrase} onChange={(event) => setEditor({ ...editor, passphrase: event.target.value })} /></label>
      </> : <label>{editor.entry.value_kind === "password" ? (zh ? "新密码" : "New password") : (zh ? "新 API key / token" : "New API key / token")}<input disabled={busy} aria-label={zh ? "新凭据值" : "New credential value"} type="password" autoComplete="new-password" value={editor.secret} onChange={(event) => setEditor({ ...editor, secret: event.target.value })} /></label>}
      <div><button disabled={busy} onClick={closeEditor}>{zh ? "取消" : "Cancel"}</button><button className="primary" disabled={busy || (editor.entry.value_kind === "ssh_private_key" ? !editor.path.trim() : !editor.secret.trim())} onClick={() => void submitReplacement()}>{busy && <LoaderCircle className="spin" size={13} />}{zh ? "替换" : "Replace"}</button></div>
    </section>}
    {confirmDelete && <section className="credential-delete-confirm" role="alertdialog" aria-label={zh ? `删除 ${confirmDelete.label}` : `Delete ${confirmDelete.label}`}>
      <Trash2 size={18} /><span><b>{zh ? `删除“${confirmDelete.label}”？` : `Delete “${confirmDelete.label}”?`}</b><small>{zh ? "这会从系统 keyring 清除秘密并删除非敏感目录条目。" : "This removes the secret from the system keyring and deletes the non-secret directory entry."}</small></span>
      <div><button disabled={busy} onClick={closeDelete}>{zh ? "取消" : "Cancel"}</button><button className="danger" disabled={busy} onClick={() => void confirmDeletion()}>{busy && <LoaderCircle className="spin" size={13} />}{zh ? "删除" : "Delete"}</button></div>
    </section>}
    {loading && entries.length === 0 ? <p className="credentials-loading"><LoaderCircle className="spin" size={15} />{zh ? "正在读取凭据目录…" : "Loading credential directory…"}</p> : entries.length === 0 ? <div className="credentials-empty"><KeyRound size={22} /><b>{zh ? "暂无凭据引用" : "No credential references"}</b><small>{zh ? "模型、SSH 或 MCP 配置保存凭据引用后会显示在这里。" : "Credentials appear here after a model, SSH, or MCP configuration stores a reference."}</small></div> : <div className="credentials-list">{entries.map((entry) => <CredentialRow key={entry.reference} entry={entry} zh={zh} busy={busy || loading} onOpenOwner={onOpenOwner} onReplace={() => { setEditor({ entry, secret: "", path: "", passphrase: "" }); setCreating(false); setConfirmDelete(null); }} onDelete={() => { setConfirmDelete(entry); setCreating(false); setEditor(null); }} />)}</div>}
    <div className="settings-note"><ShieldCheck size={18} /><span><b>{zh ? "仅影响未来解析" : "Applies to future resolution"}</b><small>{zh ? "替换或删除不会撤销服务端密钥，也不会取消已经派发的本地或远端计算。受管凭据只会在你把引用填入 MCP 环境变量时使用。" : "Replacement or deletion does not revoke a server-side key or cancel dispatched local or remote computation. A managed credential is used only when you place its reference in an MCP environment binding."}</small></span></div>
  </main>;
}

function CredentialRow({ entry, zh, busy, onOpenOwner, onReplace, onDelete }: { entry: CredentialEntry; zh: boolean; busy: boolean; onOpenOwner: (page: OwnerPage) => void; onReplace: () => void; onDelete: () => void }) {
  const owner = ownerPage(entry);
  const status = entry.presence === "present" ? (zh ? "已保存" : "Present") : entry.presence === "missing" ? (zh ? "缺失" : "Missing") : (zh ? "状态不可用" : "Unavailable");
  return <article className="credential-row">
    <header><span><b>{entry.label}</b><code>{entry.reference}</code></span><em className={`credential-presence ${entry.presence}`}>{status}</em></header>
    <small>{entry.presence === "missing" ? (zh ? "配置仍引用此账户，但 keyring 中没有秘密。可替换后重试。" : "A configuration still references this account, but no secret exists in the keyring. Replace it before retrying.") : entry.presence === "unavailable" ? (zh ? "系统 keyring 当前无法确认状态；这不表示秘密缺失。" : "The system keyring cannot confirm status right now; this does not mean the secret is missing.") : valueKindLabel(entry.value_kind, zh)}</small>
    {entry.consumers.length > 0 && <div className="credential-consumers"><b>{zh ? "用途" : "Used by"}</b>{entry.consumers.map((consumer, index) => <span key={`${consumer.kind}-${consumer.id}-${consumer.binding_name ?? index}`}>{consumerLabel(consumer, zh)}</span>)}</div>}
    {!entry.can_replace && <small className="credentials-warning">{zh ? "引用缺少规范 owner 或存在类型冲突；请在来源页面修复。" : "This reference lacks a canonical owner or has a type conflict. Repair it on its owner page."}</small>}
    <footer>{owner && <button disabled={busy} onClick={() => onOpenOwner(owner)}>{ownerLabel(owner, zh)}</button>}<button disabled={busy || !entry.can_replace} onClick={onReplace}>{zh ? "替换" : "Replace"}</button>{entry.target.kind === "managed" && (entry.can_delete ? <button disabled={busy} onClick={onDelete}>{zh ? "删除" : "Delete"}</button> : entry.consumers.length > 0 && <button disabled={busy} onClick={() => onOpenOwner("connections")}>{zh ? "查看连接" : "View connections"}</button>)}</footer>
  </article>;
}

function upsert(entries: CredentialEntry[], next: CredentialEntry) {
  return [...entries.filter((entry) => entry.reference !== next.reference), next].sort((left, right) => left.label.localeCompare(right.label));
}

function ownerPage(entry: CredentialEntry): OwnerPage | null {
  if (entry.target.kind === "model") return "models";
  if (entry.target.kind === "ssh") return "remote";
  if (entry.target.kind === "mcp_binding") return "connections";
  return null;
}

function ownerLabel(owner: OwnerPage, zh: boolean) {
  if (owner === "models") return zh ? "打开模型" : "Open models";
  if (owner === "remote") return zh ? "打开环境" : "Open environments";
  return zh ? "打开连接" : "Open connections";
}

function valueKindLabel(kind: CredentialEntry["value_kind"], zh: boolean) {
  if (kind === "ssh_private_key") return zh ? "SSH 私钥路径与可选口令" : "SSH private-key path and optional passphrase";
  if (kind === "password") return zh ? "密码" : "Password";
  return "API key / token";
}

function consumerLabel(consumer: CredentialConsumer, zh: boolean) {
  if (consumer.kind === "mcp") return `${consumer.label} · ${consumer.binding_name ?? "MCP"}`;
  if (consumer.kind === "ssh") return `${zh ? "环境" : "Environment"}: ${consumer.label}`;
  if (consumer.kind === "model") return `${zh ? "模型" : "Model"}: ${consumer.label}`;
  return consumer.label;
}
