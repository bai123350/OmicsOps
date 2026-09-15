import { useEffect, useRef, useState } from "react";

import {
  deleteSpecialistTemplate,
  listSpecialistTemplates,
  saveSpecialistTemplate,
} from "../../project-templates-api";
import type {
  SaveSpecialistTemplateRequest,
  SpecialistTemplate,
} from "../../project-template-types";
import type { WorkspaceProject } from "../../types";
import type { Locale } from "../workspace/copy";
import { useWindowEscapeLayer } from "./BrowserSettings";
import "./ProjectTemplates.css";

type SpecialistDraft = Omit<SaveSpecialistTemplateRequest, "project_id">;

export function SpecialistsSettings({
  selectedProject,
  locale,
  onChanged,
}: {
  selectedProject: WorkspaceProject | null;
  locale: Locale;
  onChanged?: () => void;
}) {
  const zh = locale === "zh-CN";
  const [templates, setTemplates] = useState<SpecialistTemplate[]>([]);
  const [loading, setLoading] = useState(false);
  const [loadError, setLoadError] = useState(false);
  const [editor, setEditor] = useState<SpecialistDraft | null>(null);
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<SpecialistTemplate | null>(null);
  const [deleting, setDeleting] = useState(false);
  const [deleteError, setDeleteError] = useState(false);
  const sequence = useRef(0);
  const saveInFlight = useRef(false);

  function closeEditor() {
    if (!saving) {
      setEditor(null);
      setSaveError(false);
    }
  }
  useWindowEscapeLayer(editor !== null, closeEditor);

  async function load(projectId: string, request: number) {
    setLoading(true);
    setLoadError(false);
    try {
      const next = await listSpecialistTemplates(projectId);
      if (request !== sequence.current) return;
      setTemplates(next);
    } catch {
      if (request === sequence.current) setLoadError(true);
    } finally {
      if (request === sequence.current) setLoading(false);
    }
  }

  useEffect(() => {
    const request = ++sequence.current;
    setTemplates([]);
    setEditor(null);
    setDeleteTarget(null);
    setSaving(false);
    setDeleting(false);
    setSaveError(false);
    setDeleteError(false);
    setLoadError(false);
    saveInFlight.current = false;
    if (!selectedProject) {
      setLoading(false);
      return;
    }
    void load(selectedProject.id, request);
    return () => { ++sequence.current; };
  }, [selectedProject?.id]);

  function newTemplate() {
    setDeleteTarget(null);
    setDeleteError(false);
    setSaveError(false);
    setEditor({ id: null, name: "", description: "", instructions: "", enabled: true });
  }

  function editTemplate(template: SpecialistTemplate) {
    setDeleteTarget(null);
    setDeleteError(false);
    setSaveError(false);
    setEditor({
      id: template.id,
      name: template.name,
      description: template.description,
      instructions: template.instructions,
      enabled: template.enabled,
    });
  }

  async function submit(event: React.FormEvent) {
    event.preventDefault();
    if (saveInFlight.current || !selectedProject || !editor || !draftValid(editor)) return;
    const request = sequence.current;
    saveInFlight.current = true;
    setSaving(true);
    setSaveError(false);
    try {
      const saved = await saveSpecialistTemplate({ ...editor, project_id: selectedProject.id });
      if (request !== sequence.current) return;
      setTemplates((current) => [...current.filter((item) => item.id !== saved.id), saved]
        .sort((left, right) => left.name.localeCompare(right.name)));
      setEditor(null);
      onChanged?.();
    } catch {
      if (request === sequence.current) setSaveError(true);
    } finally {
      if (request === sequence.current) {
        saveInFlight.current = false;
        setSaving(false);
      }
    }
  }

  async function confirmDelete() {
    if (!selectedProject || !deleteTarget) return;
    const request = sequence.current;
    const target = deleteTarget;
    setDeleting(true);
    setDeleteError(false);
    try {
      await deleteSpecialistTemplate(selectedProject.id, target.id);
      if (request !== sequence.current) return;
      setTemplates((current) => current.filter((item) => item.id !== target.id));
      setDeleteTarget(null);
      onChanged?.();
    } catch {
      if (request === sequence.current) setDeleteError(true);
    } finally {
      if (request === sequence.current) setDeleting(false);
    }
  }

  const mutationBusy = saving || deleting;
  const instructionsBytes = editor ? new TextEncoder().encode(editor.instructions).length : 0;
  return (
    <main className="project-template-settings">
      <header className="appearance-heading">
        <h3>{zh ? "专家角色" : "Specialists"}</h3>
        <p>{zh
          ? "角色模板只会把可见文本插入草稿。请检查、编辑并自行发送。"
          : "Role templates insert visible text into your draft. Review it, edit it, and send it yourself."}</p>
      </header>

      {!selectedProject ? (
        <section className="appearance-card project-template-empty">{zh ? "选择项目后才能管理该项目的角色模板。" : "Select a project to manage that project's role templates."}</section>
      ) : (
        <>
          <section className="appearance-card project-template-toolbar">
            <span><strong>{selectedProject.name}</strong><small>{zh ? `${templates.length}/100 个角色模板` : `${templates.length}/100 role templates`}</small></span>
            <button type="button" onClick={newTemplate} disabled={loading || mutationBusy || templates.length >= 100}>{zh ? "新建角色模板" : "New role template"}</button>
          </section>

          {loadError && <section className="appearance-card project-template-error" role="alert"><span>{zh ? "无法读取角色模板。" : "Could not load role templates."}</span><button type="button" onClick={() => { const request = ++sequence.current; void load(selectedProject.id, request); }}>{zh ? "重试" : "Retry"}</button></section>}
          {loading && <p role="status">{zh ? "正在读取角色模板…" : "Loading role templates…"}</p>}

          {editor && <form className="appearance-card project-template-editor" onSubmit={submit}>
            <div className="appearance-group-heading"><h4>{editor.id ? (zh ? "编辑角色模板" : "Edit role template") : (zh ? "新建角色模板" : "New role template")}</h4><p>{zh ? "保存后仍是用户可见草稿内容，不会变成系统提示或独立子 Agent。" : "The saved text remains visible draft content. It does not become a system prompt or an independent sub-agent."}</p></div>
            <fieldset disabled={saving}>
              <label><span>{zh ? "名称" : "Name"}</span><input aria-label={zh ? "角色模板名称" : "Role template name"} maxLength={100} value={editor.name} onChange={(event) => setEditor({ ...editor, name: event.target.value })} /></label>
              <label><span>{zh ? "说明" : "Description"}</span><textarea aria-label={zh ? "角色模板说明" : "Role template description"} maxLength={500} value={editor.description} onChange={(event) => setEditor({ ...editor, description: event.target.value })} /></label>
              <label><span>{zh ? "指令" : "Instructions"}</span><textarea aria-label={zh ? "指令" : "Instructions"} rows={9} value={editor.instructions} onChange={(event) => setEditor({ ...editor, instructions: event.target.value })} /></label>
              <small className={instructionsBytes > 16 * 1024 ? "project-template-error" : ""}>{new Intl.NumberFormat(locale).format(instructionsBytes)} / 16,384 UTF-8 bytes</small>
              <label className="project-template-enabled"><input type="checkbox" aria-label={zh ? "已启用" : "Enabled"} checked={editor.enabled} onChange={(event) => setEditor({ ...editor, enabled: event.target.checked })} /><span>{zh ? "启用" : "Enabled"}</span></label>
              {saveError && <p className="project-template-error" role="alert">{zh ? "无法保存角色模板。请检查字段后重试。" : "Could not save role template. Check the fields and try again."}</p>}
              <div className="project-template-actions"><button type="button" onClick={closeEditor} disabled={saving}>{zh ? "取消" : "Cancel"}</button><button type="submit" disabled={saving || !draftValid(editor)}>{saving ? (zh ? "保存中…" : "Saving…") : zh ? "保存角色模板" : "Save role template"}</button></div>
            </fieldset>
          </form>}

          {!loading && !loadError && templates.length === 0 ? <p className="project-template-empty">{zh ? "还没有角色模板。" : "No role templates yet."}</p> : <div className="project-template-list">
            {templates.map((template) => <article className="appearance-card project-template-item" key={template.id}>
              <div><span className="project-template-title"><strong>{template.name}</strong>{!template.enabled && <small>{zh ? "已禁用" : "Disabled"}</small>}</span><p>{template.description || (zh ? "无说明" : "No description")}</p><small>{zh ? "选择时会插入可见草稿文本" : "Selection inserts visible draft text"}</small></div>
              <div className="project-template-actions"><button type="button" aria-label={zh ? `编辑 ${template.name}` : `Edit ${template.name}`} onClick={() => editTemplate(template)} disabled={mutationBusy}>{zh ? "编辑" : "Edit"}</button><button type="button" aria-label={zh ? `删除 ${template.name}` : `Delete ${template.name}`} onClick={() => { setDeleteTarget(template); setDeleteError(false); }} disabled={mutationBusy}>{zh ? "删除" : "Delete"}</button></div>
              {deleteTarget?.id === template.id && <div className="project-template-delete-confirm"><span>{zh ? `删除 ${template.name}？此操作无法撤销。` : `Delete ${template.name}? This cannot be undone.`}</span><button type="button" aria-label={zh ? `确认删除 ${template.name}` : `Confirm delete ${template.name}`} onClick={() => void confirmDelete()} disabled={deleting}>{zh ? "确认删除" : "Confirm delete"}</button><button type="button" onClick={() => setDeleteTarget(null)} disabled={deleting}>{zh ? "取消" : "Cancel"}</button>{deleteError && <p role="alert">{zh ? "无法删除角色模板，请重试。" : "Could not delete role template. Please try again."}</p>}</div>}
            </article>)}
          </div>}
        </>
      )}
    </main>
  );
}

function draftValid(draft: SpecialistDraft) {
  return Boolean(
    draft.name.trim()
      && draft.instructions.trim()
      && new TextEncoder().encode(draft.instructions).length <= 16 * 1024,
  );
}
