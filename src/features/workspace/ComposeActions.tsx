import {
  BookmarkPlus,
  ChevronRight,
  ClipboardCheck,
  FilePlus2,
  FolderOpen,
  Image,
  Wrench,
} from "lucide-react";
import type { LucideIcon } from "lucide-react";

export interface ComposeActionsProps {
  zh: boolean;
  onAttach?: () => void;
  onFiles: () => void;
  onReview: () => void;
  onManageSkills?: () => void;
  onManageWorkflows?: () => void;
  onSaveSkill?: () => void;
  onShare?: () => void;
  onClose: () => void;
}

interface ComposeAction {
  id: string;
  icon: LucideIcon;
  title: string;
  description: string;
  onSelect?: () => void;
  unavailable?: boolean;
}

/**
 * Actions available from the composer plus menu.
 *
 * The menu only owns action dispatch. Its parent owns whether it is open and
 * how Escape closes it, so this component intentionally has no listeners.
 */
export function ComposeActions({ zh, onAttach, onFiles, onReview, onManageSkills, onManageWorkflows, onSaveSkill, onShare, onClose }: ComposeActionsProps) {
  const addToMessage: ComposeAction[] = [
    {
      id: "attach",
      icon: FilePlus2,
      title: zh ? "添加文件" : "Attach files",
      description: zh ? "添加图片、PDF 和数据文件" : "Add images, PDFs, and data files",
      onSelect: onAttach,
    },
    {
      id: "files",
      icon: FolderOpen,
      title: zh ? "你的文件" : "Your files",
      description: zh ? "浏览项目文件" : "Browse project files",
      onSelect: onFiles,
    },
  ];

  const session: ComposeAction[] = [
    ...(onManageWorkflows ? [{ id: "manage-workflows", icon: Wrench, title: zh ? "管理工作流" : "Manage workflows", description: zh ? "编辑项目任务步骤模板" : "Edit project task recipes", onSelect: onManageWorkflows }] : []),
    {
      id: "review",
      icon: ClipboardCheck,
      title: zh ? "请求审核" : "Request review",
      description: zh ? "请求 Agent 审核当前会话" : "Ask the Agent to review this session",
      onSelect: onReview,
    },
    {
      id: "share-image",
      icon: Image,
      title: zh ? "分享会话" : "Share conversation",
      description: onShare ? (zh ? "选择内容，脱敏并导出 PNG 或 HTML" : "Select, redact and export PNG or HTML") : (zh ? "没有可分享的内容" : "No messages to share"),
      onSelect: onShare,
    },
    {
      id: "save-skill",
      icon: BookmarkPlus,
      title: zh ? "保存为技能" : "Save as skill",
      description: onSaveSkill ? (zh ? "从当前会话准备可复用技能" : "Prepare a reusable skill from this session") : (zh ? "暂不可用" : "Unavailable"),
      onSelect: onSaveSkill,
    },
    {
      id: "manage-skills",
      icon: Wrench,
      title: zh ? "管理技能" : "Manage skills",
      description: zh ? "打开技能管理" : "Open skills management",
      onSelect: onManageSkills,
    },
  ];

  const renderAction = (action: ComposeAction) => {
    const Icon = action.icon;
    const disabled = action.unavailable || !action.onSelect;
    return (
      <button
        key={action.id}
        type="button"
        role="menuitem"
        className={`compose-action${action.unavailable ? " unavailable" : ""}`}
        disabled={disabled}
        onClick={() => {
          if (disabled || !action.onSelect) return;
          onClose();
          action.onSelect();
        }}
      >
        <span className="compose-action-icon" aria-hidden="true"><Icon size={17} strokeWidth={1.9} /></span>
        <span>
          <b>{action.title}</b>
          <small className={action.unavailable ? "compose-action-unavailable" : undefined}>{action.description}</small>
        </span>
        <ChevronRight className="compose-action-chevron" size={15} aria-hidden="true" />
      </button>
    );
  };

  return (
    <div className="composer-add-menu compose-actions" role="menu" aria-label={zh ? "撰写操作" : "Compose actions"}>
      <section className="compose-actions-group" role="group" aria-labelledby="compose-actions-add-heading">
        <b id="compose-actions-add-heading" className="compose-actions-heading">{zh ? "添加到消息" : "ADD TO MESSAGE"}</b>
        {addToMessage.map(renderAction)}
      </section>
      <section className="compose-actions-group" role="group" aria-labelledby="compose-actions-session-heading">
        <b id="compose-actions-session-heading" className="compose-actions-heading">{zh ? "会话" : "SESSION"}</b>
        {session.map(renderAction)}
      </section>
    </div>
  );
}
