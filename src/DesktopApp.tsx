import { useEffect, useState } from "react";
import * as api from "./tauri-api";
import type { WorkspaceConversation, WorkspaceProject, WorkspaceTemplate } from "./types";
import { ProjectLibrary } from "./features/projects/ProjectLibrary";
import { WorkspaceShell } from "./features/workspace/WorkspaceShell";
import type { Locale } from "./features/workspace/copy";
import { SettingsPanel } from "./features/settings/SettingsPanel";

export default function DesktopApp() {
  const [projects, setProjects] = useState<WorkspaceProject[]>([]);
  const [selected, setSelected] = useState<WorkspaceProject | null>(null);
  const [locale, setLocale] = useState<Locale>("zh-CN");
  const [loading, setLoading] = useState(true);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [conversation, setConversation] = useState<WorkspaceConversation | null>(null);
  const [messageSequence, setMessageSequence] = useState(1);

  useEffect(() => { api.listProjects().then((items) => { setProjects(items); setSelected(items[0] ?? null); }).finally(() => setLoading(false)); }, []);
  useEffect(() => {
    if (!selected) { setConversation(null); return; }
    api.listConversations(selected.id).then(async (items) => {
      const active = items[0] ?? await api.createConversation(selected.id, locale === "zh-CN" ? "QC 与聚类" : "QC and clustering");
      setConversation(active);
      const messages = await api.listMessages(active.id);
      setMessageSequence((messages.at(-1)?.sequence ?? 0) + 1);
    });
  }, [selected?.id]);
  if (loading) return <div className="desktop-loading">OmicsOps</div>;
  const settings = settingsOpen ? <SettingsPanel locale={locale} onClose={() => setSettingsOpen(false)} /> : null;
  if (!selected) return <><ProjectLibrary projects={projects} locale={locale} onLocaleChange={setLocale} onSettings={() => setSettingsOpen(true)} onOpen={setSelected} onCreate={async (template: WorkspaceTemplate, name: string) => { const localRoot = await api.chooseProjectDirectory(); if (!localRoot) return; const project = await api.createProject({ name, description: "", local_root: localRoot, template }); setProjects((current) => [project, ...current]); setSelected(project); }} />{settings}</>;
  return <><WorkspaceShell project={{ id: selected.id, name: selected.name, status: selected.status, template: selected.template }} locale={locale} onLocaleChange={setLocale} onOpenSettings={() => setSettingsOpen(true)} onSend={async (markdown) => { if (!conversation) return; await api.submitMessage({ project_id: selected.id, conversation_id: conversation.id, markdown, sequence: messageSequence }); setMessageSequence((value) => value + 1); }} />{settings}</>;
}
