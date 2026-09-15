import { useCallback, useEffect, useLayoutEffect, useRef, useState, type FormEvent, type KeyboardEvent as ReactKeyboardEvent } from "react";
import { Check, CircleAlert, LoaderCircle, Plus, Trash2, X } from "lucide-react";

import { listComposerWorkflows, saveComposerWorkflow } from "../../composer-workflow-api";
import type { ComposerWorkflowTemplate, SaveComposerWorkflowRequest } from "../../types";
import { useWindowEscapeLayer } from "../settings/BrowserSettings";
import "./workflow-library.css";

const MAX_WORKFLOW_STEPS = 12;
const MAX_STEP_BYTES = 2_000;
const MAX_WORKFLOW_STEPS_BYTES = 16 * 1024;
const MAX_WORKFLOW_NAME_CHARS = 100;
const MAX_WORKFLOW_DESCRIPTION_CHARS = 500;

const FOCUSABLE_SELECTOR = [
  "a[href]",
  "button:not([disabled])",
  "input:not([disabled])",
  "select:not([disabled])",
  "textarea:not([disabled])",
  "[contenteditable=\"true\"]",
  "[tabindex]:not([tabindex=\"-1\"])",
].join(",");

function focusableElements(container: HTMLElement | null): HTMLElement[] {
  if (!container) return [];
  return Array.from(container.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR)).filter((element) => {
    if (!element.isConnected || element.hidden || element.getAttribute("aria-hidden") === "true") return false;
    if (element.closest("fieldset[disabled]")) return false;
    if (typeof window === "undefined") return true;
    const style = window.getComputedStyle(element);
    return style.display !== "none" && style.visibility !== "hidden";
  });
}

export interface WorkflowLibraryDialogProps {
  projectId: string;
  zh: boolean;
  onClose: () => void;
  onChanged: () => void;
  presentation?: "dialog" | "embedded";
}

interface WorkflowDraft {
  id: string | null;
  name: string;
  description: string;
  steps: string[];
  enabled: boolean;
}

function newWorkflowDraft(): WorkflowDraft {
  return { id: null, name: "", description: "", steps: [""], enabled: true };
}

function draftFromTemplate(template: ComposerWorkflowTemplate): WorkflowDraft {
  return {
    id: template.id,
    name: template.name,
    description: template.description,
    steps: [...template.steps],
    enabled: template.enabled,
  };
}

function characterCount(value: string): number {
  return Array.from(value).length;
}

function utf8ByteLength(value: string): number {
  return new TextEncoder().encode(value).length;
}

function normalizedDraft(draft: WorkflowDraft): WorkflowDraft {
  return {
    ...draft,
    name: draft.name.trim(),
    description: draft.description.trim(),
    steps: draft.steps.map((step) => step.trim()),
  };
}

function validationError(draft: WorkflowDraft, zh: boolean): string | null {
  const normalized = normalizedDraft(draft);
  if (!normalized.name) return zh ? "请输入工作流名称。" : "Enter a workflow name.";
  if (characterCount(normalized.name) > MAX_WORKFLOW_NAME_CHARS) {
    return zh ? "工作流名称不能超过 100 个字符。" : "Workflow name is too long.";
  }
  if (characterCount(normalized.description) > MAX_WORKFLOW_DESCRIPTION_CHARS) {
    return zh ? "工作流描述不能超过 500 个字符。" : "Workflow description is too long.";
  }
  if (normalized.steps.length === 0) return zh ? "请至少添加一个步骤。" : "Add at least one step.";
  if (normalized.steps.length > MAX_WORKFLOW_STEPS) {
    return zh ? "工作流最多包含 12 个步骤。" : "A workflow can contain at most 12 steps.";
  }
  for (const [index, step] of normalized.steps.entries()) {
    if (!step) return zh ? `步骤 ${index + 1} 不能为空。` : `Step ${index + 1} cannot be empty.`;
    if (utf8ByteLength(step) > MAX_STEP_BYTES) {
      return zh ? `步骤 ${index + 1} 不能超过 2,000 字节。` : `Step ${index + 1} exceeds 2,000 bytes.`;
    }
  }
  const totalBytes = normalized.steps.reduce((total, step) => total + utf8ByteLength(step), 0);
  if (totalBytes > MAX_WORKFLOW_STEPS_BYTES) {
    return zh ? "步骤总大小不能超过 16 KiB。" : "Workflow steps exceed the 16 KiB total limit.";
  }
  return null;
}

function saveErrorMessage(zh: boolean): string {
  return zh ? "无法保存工作流，请重试。" : "Could not save workflow. Please try again.";
}

function loadErrorMessage(zh: boolean): string {
  return zh ? "无法加载工作流，请重试。" : "Could not load workflows. Please try again.";
}

function stepCountLabel(count: number, zh: boolean): string {
  if (zh) return `${count} 个步骤`;
  return `${count} ${count === 1 ? "step" : "steps"}`;
}

export function WorkflowLibraryDialog({ projectId, zh, onClose, onChanged, presentation = "dialog" }: WorkflowLibraryDialogProps) {
  const dialogPresentation = presentation === "dialog";
  const [workflows, setWorkflows] = useState<ComposerWorkflowTemplate[]>([]);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState("");
  const [reloadVersion, setReloadVersion] = useState(0);
  const [editor, setEditor] = useState<WorkflowDraft | null>(null);
  const [saveBusy, setSaveBusy] = useState(false);
  const [saveError, setSaveError] = useState("");
  const [savedNotice, setSavedNotice] = useState("");
  const mountedRef = useRef(true);
  const closedRef = useRef(false);
  const loadGenerationRef = useRef(0);
  const editorGenerationRef = useRef(0);
  const editorFocusIdentityRef = useRef<string | null>(null);
  const editorReturnFocusRef = useRef<HTMLElement | null>(null);
  const initialFocusRef = useRef<HTMLButtonElement>(null);
  const editorRef = useRef<HTMLElement>(null);
  const editorNameRef = useRef<HTMLInputElement>(null);
  const dialogRef = useRef<HTMLElement>(null);
  const previousFocusRef = useRef<HTMLElement | null>(null);
  const projectIdRef = useRef(projectId);
  const onCloseRef = useRef(onClose);
  const onChangedRef = useRef(onChanged);

  projectIdRef.current = projectId;
  onCloseRef.current = onClose;
  onChangedRef.current = onChanged;

  useLayoutEffect(() => {
    mountedRef.current = true;
    if (dialogPresentation && previousFocusRef.current === null && typeof document !== "undefined") {
      previousFocusRef.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    }
    if (dialogPresentation) initialFocusRef.current?.focus();
    return () => {
      mountedRef.current = false;
      loadGenerationRef.current += 1;
      editorGenerationRef.current += 1;
      const previous = previousFocusRef.current;
      const active = typeof document === "undefined" ? null : document.activeElement;
      const dialog = dialogRef.current;
      const focusIsInDialog = Boolean(dialog && active && dialog.contains(active));
      const focusIsUnowned = active === null || (typeof document !== "undefined" && active === document.body);
      if (dialogPresentation && previous?.isConnected && (focusIsInDialog || focusIsUnowned)) previous.focus();
    };
  }, [dialogPresentation]);

  const editorFocusIdentity = editor ? (editor.id ?? "__new_workflow__") : null;
  useLayoutEffect(() => {
    const previousIdentity = editorFocusIdentityRef.current;
    if (editorFocusIdentity !== null && editorFocusIdentity !== previousIdentity) {
      editorNameRef.current?.focus();
    } else if (editorFocusIdentity === null && previousIdentity !== null) {
      const active = typeof document === "undefined" ? null : document.activeElement;
      const editor = editorRef.current;
      const dialog = dialogRef.current;
      const focusIsInEditor = Boolean(editor && active && editor.contains(active));
      const focusIsUnowned = active === null || (typeof document !== "undefined" && active === document.body);
      const target = editorReturnFocusRef.current;
      const available = focusableElements(dialog);
      if ((focusIsInEditor || focusIsUnowned) && target && available.includes(target)) target.focus();
      else if ((focusIsInEditor || focusIsUnowned) && initialFocusRef.current) initialFocusRef.current.focus();
      editorReturnFocusRef.current = null;
    }
    editorFocusIdentityRef.current = editorFocusIdentity;
  }, [editorFocusIdentity]);

  useEffect(() => {
    const generation = ++loadGenerationRef.current;
    editorGenerationRef.current += 1;
    setWorkflows([]);
    setEditor(null);
    setSaveBusy(false);
    setSaveError("");
    setSavedNotice("");
    setLoading(true);
    setLoadError("");

    void listComposerWorkflows(projectId)
      .then((result) => {
        if (!mountedRef.current || closedRef.current || generation !== loadGenerationRef.current) return;
        setWorkflows(result.filter((workflow) => workflow.project_id === projectId));
      })
      .catch(() => {
        if (!mountedRef.current || closedRef.current || generation !== loadGenerationRef.current) return;
        setLoadError(loadErrorMessage(zh));
      })
      .finally(() => {
        if (!mountedRef.current || closedRef.current || generation !== loadGenerationRef.current) return;
        setLoading(false);
      });
  }, [projectId, reloadVersion, zh]);

  const closeLibrary = useCallback(() => {
    if (closedRef.current) return;
    closedRef.current = true;
    loadGenerationRef.current += 1;
    editorGenerationRef.current += 1;
    setEditor(null);
    setSaveBusy(false);
    setSaveError("");
    onCloseRef.current();
  }, []);

  const closeEditor = useCallback(() => {
    editorGenerationRef.current += 1;
    setEditor(null);
    setSaveBusy(false);
    setSaveError("");
  }, []);

  useWindowEscapeLayer(dialogPresentation, closeLibrary);
  useWindowEscapeLayer(editor !== null, closeEditor);

  function rememberEditorLaunchFocus(trigger?: HTMLElement) {
    const candidate = trigger ?? (typeof document !== "undefined" && document.activeElement instanceof HTMLElement ? document.activeElement : null);
    editorReturnFocusRef.current = candidate && dialogRef.current?.contains(candidate)
      ? candidate
      : initialFocusRef.current;
  }

  function startNewWorkflow(trigger?: HTMLElement) {
    rememberEditorLaunchFocus(trigger);
    editorGenerationRef.current += 1;
    setEditor(newWorkflowDraft());
    setSaveBusy(false);
    setSaveError("");
    setSavedNotice("");
  }

  function editWorkflow(workflow: ComposerWorkflowTemplate, trigger?: HTMLElement) {
    rememberEditorLaunchFocus(trigger);
    editorGenerationRef.current += 1;
    setEditor(draftFromTemplate(workflow));
    setSaveBusy(false);
    setSaveError("");
    setSavedNotice("");
  }

  function updateEditor(update: (current: WorkflowDraft) => WorkflowDraft) {
    setEditor((current) => current ? update(current) : current);
    setSaveError("");
  }

  function updateStep(index: number, value: string) {
    updateEditor((current) => ({
      ...current,
      steps: current.steps.map((step, stepIndex) => stepIndex === index ? value : step),
    }));
  }

  function addStep() {
    if (!editor || editor.steps.length >= MAX_WORKFLOW_STEPS || saveBusy) return;
    updateEditor((current) => ({ ...current, steps: [...current.steps, ""] }));
  }

  function removeStep(index: number) {
    if (!editor || saveBusy) return;
    updateEditor((current) => ({ ...current, steps: current.steps.filter((_, stepIndex) => stepIndex !== index) }));
  }

  async function submitWorkflow(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!editor || saveBusy) return;
    const error = validationError(editor, zh);
    if (error) {
      setSaveError(error);
      return;
    }

    const draft = normalizedDraft(editor);
    const loadGeneration = loadGenerationRef.current;
    const editorGeneration = editorGenerationRef.current;
    const request: SaveComposerWorkflowRequest = {
      id: draft.id,
      project_id: projectId,
      name: draft.name,
      description: draft.description,
      steps: draft.steps,
      enabled: draft.enabled,
    };
    setSaveBusy(true);
    setSaveError("");

    try {
      const saved = await saveComposerWorkflow(request);
      const currentLibrary = mountedRef.current
        && !closedRef.current
        && projectIdRef.current === projectId
        && loadGeneration === loadGenerationRef.current;
      if (!currentLibrary) return;
      const currentEditor = editorGeneration === editorGenerationRef.current;
      if (saved.project_id !== projectId) {
        if (currentEditor) setSaveError(saveErrorMessage(zh));
        return;
      }
      setWorkflows((currentWorkflows) => {
        const existingIndex = currentWorkflows.findIndex((workflow) => workflow.id === saved.id);
        if (existingIndex < 0) return [...currentWorkflows, saved];
        return currentWorkflows.map((workflow, index) => index === existingIndex ? saved : workflow);
      });
      if (!currentEditor) {
        setSavedNotice(zh ? "工作流已保存。" : "Workflow saved.");
        onChangedRef.current();
        return;
      }
      editorGenerationRef.current += 1;
      setEditor(null);
      setSaveBusy(false);
      setSaveError("");
      setSavedNotice(zh ? "工作流已保存。" : "Workflow saved.");
      onChangedRef.current();
    } catch {
      if (mountedRef.current && !closedRef.current && projectIdRef.current === projectId && loadGeneration === loadGenerationRef.current && editorGeneration === editorGenerationRef.current) {
        setSaveError(saveErrorMessage(zh));
      }
    } finally {
      if (mountedRef.current && !closedRef.current && projectIdRef.current === projectId && loadGeneration === loadGenerationRef.current && editorGeneration === editorGenerationRef.current) {
        setSaveBusy(false);
      }
    }
  }

  function handleDialogKeyDown(event: ReactKeyboardEvent<HTMLElement>) {
    if (event.key !== "Tab") return;
    const target = event.target;
    if (!(target instanceof Node)) return;
    const nestedEditor = editorRef.current;
    const scope = nestedEditor?.contains(target) ? nestedEditor : dialogRef.current;
    const focusables = focusableElements(scope);
    if (!scope || focusables.length === 0) {
      event.preventDefault();
      return;
    }
    const active = typeof document !== "undefined" && document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const index = active ? focusables.indexOf(active) : -1;
    const direction = event.shiftKey ? -1 : 1;
    const nextIndex = index < 0 ? (event.shiftKey ? focusables.length - 1 : 0) : (index + direction + focusables.length) % focusables.length;
    event.preventDefault();
    focusables[nextIndex].focus();
  }

  const title = zh ? "工作流库" : "Workflow library";
  const description = zh ? "保存可附加到 Agent 或计划的有序任务配方。" : "Save ordered task recipes to attach to an Agent or Plan.";
  const loadingLabel = zh ? "正在加载工作流" : "Loading workflows";
  const emptyLabel = zh ? "还没有保存的工作流" : "No saved workflows";
  const editorTitle = editor?.id ? (zh ? "编辑工作流" : "Edit workflow") : (zh ? "新建工作流" : "New workflow");

  return (
    <div className={dialogPresentation ? "workflow-library-backdrop" : "workflow-library-embedded"}>
      <section ref={dialogRef} className="workflow-library-dialog" role={dialogPresentation ? "dialog" : "region"} aria-modal={dialogPresentation || undefined} aria-labelledby="workflow-library-title" aria-busy={loading || saveBusy || undefined} onKeyDown={dialogPresentation ? handleDialogKeyDown : undefined}>
        <header className="workflow-library-header">
          <div>
            <span className="workflow-library-kicker">OmicsOps</span>
            <h2 id="workflow-library-title">{title}</h2>
            <p>{description}</p>
          </div>
          {dialogPresentation && <button ref={initialFocusRef} type="button" className="workflow-library-close" aria-label={zh ? "关闭工作流库" : "Close workflow library"} onClick={closeLibrary}>
            <X size={18} />
          </button>}
        </header>

        <div className="workflow-library-body">
          <section className="workflow-library-catalog" aria-labelledby="workflow-library-catalog-title">
            <div className="workflow-library-section-heading">
              <div>
                <span className="workflow-library-eyebrow">{zh ? "项目模板" : "PROJECT RECIPES"}</span>
                <h3 id="workflow-library-catalog-title">{zh ? "已保存的工作流" : "Saved workflows"}</h3>
              </div>
              <button type="button" className="workflow-library-new" onClick={(event) => startNewWorkflow(event.currentTarget)} disabled={saveBusy}>
                <Plus size={15} aria-hidden="true" />{zh ? "新建工作流" : "New workflow"}
              </button>
            </div>

            {loading && <div className="workflow-library-state" role="status" aria-label={loadingLabel}><LoaderCircle size={17} className="workflow-library-spinner" aria-hidden="true" />{loadingLabel}</div>}
            {loadError && <div className="workflow-library-state workflow-library-error" role="alert"><CircleAlert size={17} aria-hidden="true" /><span>{loadError}</span><button type="button" onClick={() => setReloadVersion((version) => version + 1)}>{zh ? "重试" : "Retry"}</button></div>}
            {!loading && !loadError && workflows.length === 0 && <div className="workflow-library-empty" role="status"><strong>{emptyLabel}</strong><span>{zh ? "创建一个有序配方，并在撰写器中附加到 Agent 或计划。" : "Create an ordered recipe to attach from the composer to an Agent or Plan."}</span></div>}
            {workflows.length > 0 && <ul className="workflow-library-list" aria-label={zh ? "工作流列表" : "Workflow list"}>
              {workflows.map((workflow) => (
                <li className="workflow-library-card" key={workflow.id}>
                  <div className="workflow-library-card-copy">
                    <div className="workflow-library-card-title">
                      <strong>{workflow.name}</strong>
                      <span className={`workflow-library-status${workflow.enabled ? " is-enabled" : ""}`}>{workflow.enabled ? (zh ? "已启用" : "Enabled") : (zh ? "已停用" : "Disabled")}</span>
                    </div>
                    {workflow.description && <p>{workflow.description}</p>}
                    <small>{stepCountLabel(workflow.steps.length, zh)}</small>
                  </div>
                  <button type="button" className="workflow-library-edit" aria-label={zh ? `编辑 ${workflow.name}` : `Edit ${workflow.name}`} onClick={(event) => editWorkflow(workflow, event.currentTarget)} disabled={saveBusy}>
                    {zh ? "编辑" : "Edit"}
                  </button>
                </li>
              ))}
            </ul>}
            {savedNotice && <p className="workflow-library-saved" role="status"><Check size={15} aria-hidden="true" />{savedNotice}</p>}
          </section>

          {editor && <section ref={editorRef} className="workflow-library-editor" aria-labelledby="workflow-library-editor-title">
            <div className="workflow-library-editor-heading">
              <div>
                <span className="workflow-library-eyebrow">{zh ? "编辑器" : "EDITOR"}</span>
                <h3 id="workflow-library-editor-title">{editorTitle}</h3>
              </div>
              <button type="button" className="workflow-library-editor-close" aria-label={zh ? "关闭编辑器" : "Close editor"} onClick={closeEditor} disabled={saveBusy}><X size={17} /></button>
            </div>

            <form className="workflow-library-form" onSubmit={(event) => void submitWorkflow(event)}>
              <fieldset disabled={saveBusy}>
                <label className="workflow-library-field">
                  <span>{zh ? "名称" : "Name"}</span>
                    <input
                    ref={editorNameRef}
                    aria-label={zh ? "工作流名称" : "Workflow name"}
                    value={editor.name}
                    maxLength={MAX_WORKFLOW_NAME_CHARS}
                    onChange={(event) => updateEditor((current) => ({ ...current, name: event.target.value }))}
                  />
                </label>
                <label className="workflow-library-field">
                  <span>{zh ? "描述" : "Description"}</span>
                  <textarea
                    aria-label={zh ? "工作流描述" : "Workflow description"}
                    value={editor.description}
                    maxLength={MAX_WORKFLOW_DESCRIPTION_CHARS}
                    rows={3}
                    onChange={(event) => updateEditor((current) => ({ ...current, description: event.target.value }))}
                  />
                </label>

                <div className="workflow-library-steps-heading">
                  <div>
                    <span>{zh ? "有序步骤" : "Ordered steps"}</span>
                    <small>{zh ? "每步最多 2,000 字节，全部步骤最多 16 KiB。" : "Up to 2,000 bytes per step and 16 KiB across all steps."}</small>
                  </div>
                  <button type="button" className="workflow-library-add-step" onClick={addStep} disabled={editor.steps.length >= MAX_WORKFLOW_STEPS}>
                    <Plus size={14} aria-hidden="true" />{zh ? "添加步骤" : "Add step"}
                  </button>
                </div>
                {editor.steps.length > 0 ? <ol className="workflow-library-steps">
                  {editor.steps.map((step, index) => (
                    <li key={`${editor.id ?? "new"}-${index}`}>
                      <span className="workflow-library-step-number" aria-hidden="true">{index + 1}</span>
                      <textarea aria-label={zh ? `步骤 ${index + 1}` : `Step ${index + 1}`} value={step} maxLength={MAX_STEP_BYTES} rows={2} onChange={(event) => updateStep(index, event.target.value)} />
                      <button type="button" className="workflow-library-remove-step" aria-label={zh ? `删除步骤 ${index + 1}` : `Remove step ${index + 1}`} onClick={() => removeStep(index)}>
                        <Trash2 size={15} aria-hidden="true" />
                      </button>
                    </li>
                  ))}
                </ol> : <p className="workflow-library-no-steps">{zh ? "尚未添加步骤。" : "No steps yet."}</p>}

                <label className="workflow-library-enabled">
                  <input type="checkbox" aria-label={zh ? "启用" : "Enabled"} checked={editor.enabled} onChange={(event) => updateEditor((current) => ({ ...current, enabled: event.target.checked }))} />
                  <span><strong>{zh ? "启用工作流" : "Enabled"}</strong><small>{zh ? "启用的工作流会出现在撰写器引用选择器中。" : "Enabled workflows appear in the composer reference picker."}</small></span>
                </label>
              </fieldset>
              {saveError && <p className="workflow-library-form-error" role="alert">{saveError}</p>}
              <div className="workflow-library-form-actions">
                <button type="button" className="workflow-library-secondary" onClick={closeEditor} disabled={saveBusy}>{zh ? "取消" : "Cancel"}</button>
                <button type="submit" className="workflow-library-primary" disabled={saveBusy}>{saveBusy ? (zh ? "保存中…" : "Saving…") : (zh ? "保存工作流" : "Save workflow")}</button>
              </div>
            </form>
          </section>}
        </div>
      </section>
    </div>
  );
}
