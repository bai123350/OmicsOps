import { useEffect, useMemo, useRef, useState } from "react";

import { listComposerWorkflows } from "../../composer-workflow-api";
import {
  listQuickActions,
  listSpecialistTemplates,
} from "../../project-templates-api";
import type { QuickAction, SpecialistTemplate } from "../../project-template-types";
import type { ComposerWorkflowTemplate, WorkspaceProject } from "../../types";
import type { Locale } from "./copy";
import { useWindowEscapeLayer } from "../settings/BrowserSettings";
import "./project-template-picker.css";

export function ProjectTemplatePicker({
  selectedProject,
  locale,
  onSelectWorkflow,
  onSelectSpecialist,
  onClose,
}: {
  selectedProject: Pick<WorkspaceProject, "id" | "name"> | null;
  locale: Locale;
  onSelectWorkflow: (workflow: ComposerWorkflowTemplate) => void;
  onSelectSpecialist: (template: SpecialistTemplate) => void;
  onClose: () => void;
}) {
  const zh = locale === "zh-CN";
  const [actions, setActions] = useState<QuickAction[]>([]);
  const [specialists, setSpecialists] = useState<SpecialistTemplate[]>([]);
  const [workflows, setWorkflows] = useState<ComposerWorkflowTemplate[]>([]);
  const [loading, setLoading] = useState(false);
  const [loadError, setLoadError] = useState(false);
  const sequence = useRef(0);
  const selecting = useRef(false);
  const dialogRef = useRef<HTMLDivElement>(null);
  const closeRef = useRef<HTMLButtonElement>(null);
  const previousFocus = useRef<HTMLElement | null>(null);

  useWindowEscapeLayer(true, onClose);

  async function load(projectId: string, request: number) {
    setLoading(true);
    setLoadError(false);
    try {
      const [nextActions, nextSpecialists, nextWorkflows] = await Promise.all([
        listQuickActions(projectId),
        listSpecialistTemplates(projectId),
        listComposerWorkflows(projectId),
      ]);
      if (request !== sequence.current) return;
      setActions(nextActions);
      setSpecialists(nextSpecialists);
      setWorkflows(nextWorkflows);
    } catch {
      if (request === sequence.current) setLoadError(true);
    } finally {
      if (request === sequence.current) setLoading(false);
    }
  }

  useEffect(() => {
    const request = ++sequence.current;
    selecting.current = false;
    setActions([]);
    setSpecialists([]);
    setWorkflows([]);
    setLoadError(false);
    if (!selectedProject) {
      setLoading(false);
      return;
    }
    void load(selectedProject.id, request);
    return () => { ++sequence.current; };
  }, [selectedProject?.id]);

  useEffect(() => {
    previousFocus.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    closeRef.current?.focus();
    return () => previousFocus.current?.focus();
  }, []);

  const workflowsById = useMemo(
    () => new Map(workflows.map((workflow) => [workflow.id, workflow])),
    [workflows],
  );

  function actionWorkflow(action: QuickAction) {
    const workflow = workflowsById.get(action.workflow_id);
    return action.enabled
      && action.project_id === selectedProject?.id
      && workflow?.enabled
      && workflow.project_id === selectedProject.id
      ? workflow
      : null;
  }

  function selectWorkflow(action: QuickAction) {
    const workflow = actionWorkflow(action);
    if (!workflow || selecting.current) return;
    selecting.current = true;
    onSelectWorkflow(workflow);
    onClose();
  }

  function selectSpecialist(template: SpecialistTemplate) {
    if (!template.enabled || template.project_id !== selectedProject?.id || selecting.current) return;
    selecting.current = true;
    onSelectSpecialist(template);
    onClose();
  }

  function trapFocus(event: React.KeyboardEvent) {
    if (event.key !== "Tab") return;
    const focusables = Array.from(dialogRef.current?.querySelectorAll<HTMLElement>(
      'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])',
    ) ?? []).filter((element) => element.getClientRects().length > 0 || element === document.activeElement);
    if (!focusables.length) return;
    const current = document.activeElement instanceof HTMLElement ? focusables.indexOf(document.activeElement) : -1;
    const next = current < 0
      ? (event.shiftKey ? focusables.length - 1 : 0)
      : (current + (event.shiftKey ? -1 : 1) + focusables.length) % focusables.length;
    event.preventDefault();
    focusables[next].focus();
  }

  return <div className="project-template-picker-backdrop">
    <div ref={dialogRef} className="project-template-picker" role="dialog" aria-modal="true" aria-label={zh ? "项目模板选择器" : "Project template picker"} onKeyDown={trapFocus}>
      <header>
        <div><h3>{zh ? "插入项目模板" : "Insert project template"}</h3><p>{zh ? "选择只会更新当前草稿，不会发送、运行工作流或启动子 Agent。" : "Selection only updates the current draft. It does not send, run a workflow, or start a sub-agent."}</p></div>
        <button ref={closeRef} type="button" aria-label={zh ? "关闭项目模板选择器" : "Close project template picker"} onClick={onClose}>×</button>
      </header>

      {!selectedProject ? <p className="project-template-picker-empty">{zh ? "选择项目后才能使用项目快捷操作和角色模板。" : "Select a project to use project quick actions and role templates."}</p> : <>
        {loading && <p role="status">{zh ? "正在读取项目模板…" : "Loading project templates…"}</p>}
        {loadError && <div className="project-template-picker-error" role="alert"><span>{zh ? "无法读取项目模板，请重试。" : "Could not load project templates. Please try again."}</span><button type="button" onClick={() => { const request = ++sequence.current; void load(selectedProject.id, request); }}>{zh ? "重试" : "Retry"}</button></div>}
        {!loading && !loadError && <div className="project-template-picker-columns">
          <section aria-labelledby="project-template-actions-heading">
            <div className="project-template-picker-heading"><h4 id="project-template-actions-heading">{zh ? "快捷操作" : "Quick Actions"}</h4><p>{zh ? "插入现有已启用工作流的引用。" : "Insert a reference to an existing enabled workflow."}</p></div>
            {actions.length === 0 ? <p className="project-template-picker-empty">{zh ? "此项目没有快捷操作。" : "No quick actions in this project."}</p> : <ul>{actions.map((action) => {
              const available = Boolean(actionWorkflow(action));
              return <li key={action.id}><button type="button" disabled={!available} aria-label={available ? (zh ? `使用 ${action.name}` : `Use ${action.name}`) : (zh ? `${action.name} 不可用` : `${action.name} unavailable`)} onClick={() => selectWorkflow(action)}><span><strong>{action.name}</strong><small>{action.description || (zh ? "无说明" : "No description")}</small></span><b>{available ? (zh ? "插入" : "Insert") : (zh ? "不可用" : "Unavailable")}</b></button></li>;
            })}</ul>}
          </section>

          <section aria-labelledby="project-template-specialists-heading">
            <div className="project-template-picker-heading"><h4 id="project-template-specialists-heading">{zh ? "专家角色" : "Specialists"}</h4><p>{zh ? "把可见角色说明交给输入框追加到草稿。" : "Pass visible role instructions to the composer for draft insertion."}</p></div>
            <p className="project-template-picker-note">{zh ? "添加可见文本到草稿；不会启动子 Agent。" : "Adds visible text to your draft; it does not start a sub-agent."}</p>
            {specialists.length === 0 ? <p className="project-template-picker-empty">{zh ? "此项目没有角色模板。" : "No role templates in this project."}</p> : <ul>{specialists.map((template) => {
              const available = template.enabled && template.project_id === selectedProject.id;
              return <li key={template.id}><button type="button" disabled={!available} aria-label={available ? (zh ? `使用 ${template.name}` : `Use ${template.name}`) : (zh ? `${template.name} 不可用` : `${template.name} unavailable`)} onClick={() => selectSpecialist(template)}><span><strong>{template.name}</strong><small>{template.description || (zh ? "无说明" : "No description")}</small></span><b>{available ? (zh ? "插入" : "Insert") : (zh ? "不可用" : "Unavailable")}</b></button></li>;
            })}</ul>}
          </section>
        </div>}
      </>}
    </div>
  </div>;
}
