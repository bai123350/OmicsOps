import { useEffect, useMemo, useRef, useState } from "react";

import { listComposerWorkflows } from "../../composer-workflow-api";
import {
  deleteQuickAction,
  listQuickActions,
  saveQuickAction,
} from "../../project-templates-api";
import type { QuickAction, SaveQuickActionRequest } from "../../project-template-types";
import type { ComposerWorkflowTemplate, WorkspaceProject } from "../../types";
import type { Locale } from "../workspace/copy";
import { useWindowEscapeLayer } from "./BrowserSettings";
import "./ProjectTemplates.css";

type ActionDraft = Omit<SaveQuickActionRequest, "project_id">;

export function QuickActionsSettings({
  selectedProject,
  locale,
  onChanged,
}: {
  selectedProject: WorkspaceProject | null;
  locale: Locale;
  onChanged?: () => void;
}) {
  const zh = locale === "zh-CN";
  const [actions, setActions] = useState<QuickAction[]>([]);
  const [workflows, setWorkflows] = useState<ComposerWorkflowTemplate[]>([]);
  const [loading, setLoading] = useState(false);
  const [loadError, setLoadError] = useState(false);
  const [editor, setEditor] = useState<ActionDraft | null>(null);
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<QuickAction | null>(null);
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
      const [nextActions, nextWorkflows] = await Promise.all([
        listQuickActions(projectId),
        listComposerWorkflows(projectId),
      ]);
      if (request !== sequence.current) return;
      setActions(nextActions);
      setWorkflows(nextWorkflows);
    } catch {
      if (request === sequence.current) setLoadError(true);
    } finally {
      if (request === sequence.current) setLoading(false);
    }
  }

  useEffect(() => {
    const request = ++sequence.current;
    setActions([]);
    setWorkflows([]);
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

  const workflowsById = useMemo(
    () => new Map(workflows.map((workflow) => [workflow.id, workflow])),
    [workflows],
  );
  const availableWorkflows = workflows.filter(
    (workflow) => workflow.enabled && workflow.project_id === selectedProject?.id,
  );

  function newAction() {
    setDeleteTarget(null);
    setDeleteError(false);
    setSaveError(false);
    setEditor({
      id: null,
      name: "",
      description: "",
      workflow_id: availableWorkflows[0]?.id ?? "",
      enabled: true,
    });
  }

  function editAction(action: QuickAction) {
    setDeleteTarget(null);
    setDeleteError(false);
    setSaveError(false);
    setEditor({
      id: action.id,
      name: action.name,
      description: action.description,
      workflow_id: action.workflow_id,
      enabled: action.enabled,
    });
  }

  async function submit(event: React.FormEvent) {
    event.preventDefault();
    if (saveInFlight.current || !selectedProject || !editor || !workflowAvailable(editor.workflow_id)) return;
    const request = sequence.current;
    saveInFlight.current = true;
    setSaving(true);
    setSaveError(false);
    try {
      const saved = await saveQuickAction({ ...editor, project_id: selectedProject.id });
      if (request !== sequence.current) return;
      setActions((current) => [...current.filter((item) => item.id !== saved.id), saved]
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
      await deleteQuickAction(selectedProject.id, target.id);
      if (request !== sequence.current) return;
      setActions((current) => current.filter((item) => item.id !== target.id));
      setDeleteTarget(null);
      onChanged?.();
    } catch {
      if (request === sequence.current) setDeleteError(true);
    } finally {
      if (request === sequence.current) setDeleting(false);
    }
  }

  function workflowAvailable(workflowId: string) {
    const workflow = workflowsById.get(workflowId);
    return Boolean(workflow?.enabled && workflow.project_id === selectedProject?.id);
  }

  const mutationBusy = saving || deleting;
  return (
    <main className="project-template-settings">
      <header className="appearance-heading">
        <h3>{zh ? "快捷操作" : "Quick Actions"}</h3>
        <p>{zh
          ? "把当前项目中已启用的工作流保存为快捷入口。选择快捷操作只会插入工作流引用，由你决定何时发送。"
          : "Save enabled workflows in this project as shortcuts. Choosing a quick action only inserts its workflow reference; you decide when to send."}</p>
      </header>

      {!selectedProject ? (
        <section className="appearance-card project-template-empty">
          {zh ? "选择项目后才能管理绑定到该项目工作流的快捷操作。" : "Select a project to manage quick actions bound to that project's workflows."}
        </section>
      ) : (
        <>
          <section className="appearance-card project-template-toolbar">
            <span><strong>{selectedProject.name}</strong><small>{zh ? `${actions.length}/100 个快捷操作` : `${actions.length}/100 quick actions`}</small></span>
            <button type="button" onClick={newAction} disabled={mutationBusy || actions.length >= 100}>{zh ? "新建快捷操作" : "New quick action"}</button>
          </section>

          {loadError && <section className="appearance-card project-template-error" role="alert">
            <span>{zh ? "无法读取快捷操作和工作流。" : "Could not load quick actions and workflows."}</span>
            <button type="button" onClick={() => { const request = ++sequence.current; void load(selectedProject.id, request); }}>{zh ? "重试" : "Retry"}</button>
          </section>}
          {loading && <p role="status">{zh ? "正在读取快捷操作…" : "Loading quick actions…"}</p>}

          {editor && <form className="appearance-card project-template-editor" onSubmit={submit}>
            <div className="appearance-group-heading"><h4>{editor.id ? (zh ? "编辑快捷操作" : "Edit quick action") : (zh ? "新建快捷操作" : "New quick action")}</h4></div>
            <fieldset disabled={saving}>
            <label><span>{zh ? "名称" : "Name"}</span><input aria-label={zh ? "快捷操作名称" : "Quick action name"} maxLength={100} value={editor.name} onChange={(event) => setEditor({ ...editor, name: event.target.value })} /></label>
            <label><span>{zh ? "说明" : "Description"}</span><textarea aria-label={zh ? "快捷操作说明" : "Quick action description"} maxLength={500} value={editor.description} onChange={(event) => setEditor({ ...editor, description: event.target.value })} /></label>
            <label><span>{zh ? "工作流" : "Workflow"}</span>
              <select aria-label={zh ? "工作流" : "Workflow"} aria-describedby={!workflowAvailable(editor.workflow_id) && editor.workflow_id ? "quick-action-workflow-unavailable" : undefined} value={editor.workflow_id} onChange={(event) => setEditor({ ...editor, workflow_id: event.target.value })}>
                {!editor.workflow_id && <option value="">{zh ? "选择已启用的工作流" : "Select an enabled workflow"}</option>}
                {editor.workflow_id && !workflowsById.has(editor.workflow_id) && <option value={editor.workflow_id} disabled>{zh ? "工作流不存在" : "Workflow missing"}</option>}
                {workflows.map((workflow) => <option key={workflow.id} value={workflow.id} disabled={!workflow.enabled || workflow.project_id !== selectedProject.id}>{workflow.name}{workflow.enabled ? "" : zh ? "（已禁用）" : " (disabled)"}</option>)}
              </select>
            </label>
            {editor.workflow_id && !workflowAvailable(editor.workflow_id) && <small id="quick-action-workflow-unavailable">{zh ? "所选工作流不可用或已禁用。" : "The selected workflow is unavailable or disabled."}</small>}
            <label className="project-template-enabled"><input type="checkbox" aria-label={zh ? "已启用" : "Enabled"} checked={editor.enabled} onChange={(event) => setEditor({ ...editor, enabled: event.target.checked })} /><span>{zh ? "启用" : "Enabled"}</span></label>
            {saveError && <p className="project-template-error" role="alert">{zh ? "无法保存快捷操作。请检查字段后重试。" : "Could not save quick action. Check the fields and try again."}</p>}
            <div className="project-template-actions"><button type="button" onClick={closeEditor} disabled={saving}>{zh ? "取消" : "Cancel"}</button><button type="submit" disabled={saving || !editor.name.trim() || !workflowAvailable(editor.workflow_id)}>{saving ? (zh ? "保存中…" : "Saving…") : zh ? "保存快捷操作" : "Save quick action"}</button></div>
            </fieldset>
          </form>}

          {!loading && !loadError && actions.length === 0 ? <p className="project-template-empty">{zh ? "还没有快捷操作。" : "No quick actions yet."}</p> : <div className="project-template-list">
            {actions.map((item) => {
              const boundWorkflow = workflowsById.get(item.workflow_id);
              const available = workflowAvailable(item.workflow_id);
              return <article className="appearance-card project-template-item" key={item.id}>
                <div><span className="project-template-title"><strong>{item.name}</strong>{!item.enabled && <small>{zh ? "已禁用" : "Disabled"}</small>}</span><p>{item.description || (zh ? "无说明" : "No description")}</p><small>{available ? boundWorkflow!.name : zh ? "工作流不可用或已禁用" : "Workflow unavailable or disabled"}</small></div>
                <div className="project-template-actions"><button type="button" aria-label={zh ? `编辑 ${item.name}` : `Edit ${item.name}`} onClick={() => editAction(item)} disabled={mutationBusy}>{zh ? "编辑" : "Edit"}</button><button type="button" aria-label={zh ? `删除 ${item.name}` : `Delete ${item.name}`} onClick={() => { setDeleteTarget(item); setDeleteError(false); }} disabled={mutationBusy}>{zh ? "删除" : "Delete"}</button></div>
                {deleteTarget?.id === item.id && <div className="project-template-delete-confirm"><span>{zh ? `删除 ${item.name}？此操作无法撤销。` : `Delete ${item.name}? This cannot be undone.`}</span><button type="button" aria-label={zh ? `确认删除 ${item.name}` : `Confirm delete ${item.name}`} onClick={() => void confirmDelete()} disabled={deleting}>{zh ? "确认删除" : "Confirm delete"}</button><button type="button" onClick={() => setDeleteTarget(null)} disabled={deleting}>{zh ? "取消" : "Cancel"}</button>{deleteError && <p role="alert">{zh ? "无法删除快捷操作，请重试。" : "Could not delete quick action. Please try again."}</p>}</div>}
              </article>;
            })}
          </div>}
        </>
      )}
    </main>
  );
}
