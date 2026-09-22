import { useEffect, useId, useLayoutEffect, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { Dna, X } from "lucide-react";
import { version } from "../../../package.json";
import { useAppearance, type AppearanceTheme } from "../../use-appearance";
import { useWindowEscapeLayer } from "../settings/BrowserSettings";
import type { Locale } from "../workspace/copy";
import type { SettingsSection } from "../settings/SettingsPanel";
import "./application-menu.css";

export interface ApplicationMenuBarProps {
  locale: Locale;
  hasProject: boolean;
  newConversationDisabled?: boolean;
  dismissRequest?: number;
  onNewProject: () => void;
  onNewConversation?: () => void | Promise<void>;
  onProjects: () => void;
  onSearch: () => void;
  onSettings: (section: SettingsSection) => void;
  onFiles: () => void;
  onError?: (message: string) => void;
  children?: ReactNode;
}

type MenuCommand = {
  label: string;
  action: () => void | Promise<void>;
  disabled?: boolean;
  shortcut?: string;
  theme?: AppearanceTheme;
  separatorBefore?: boolean;
};

const MENU_NAMES = ["File", "Edit", "View", "Help"];

export function ApplicationMenuBar(props: ApplicationMenuBarProps) {
  const { locale, hasProject, newConversationDisabled, onNewProject, onNewConversation, onProjects, onSearch, onSettings, onFiles, onError, children } = props;
  const zh = locale === "zh-CN";
  const { theme, setTheme } = useAppearance();
  const [openMenu, setOpenMenu] = useState<number | null>(null);
  const [activeTrigger, setActiveTrigger] = useState(0);
  const [help, setHelp] = useState<"quick-start" | "about" | null>(null);
  const navigationRef = useRef<HTMLElement>(null);
  const triggerRefs = useRef<Array<HTMLButtonElement | null>>([]);
  const menuRef = useRef<HTMLDivElement>(null);
  const helpCloseRef = useRef<HTMLButtonElement>(null);
  const pendingFocus = useRef<"first" | "last" | null>(null);
  const id = useId();

  function closeMenu(restoreFocus = true) {
    if (restoreFocus && openMenu !== null) triggerRefs.current[openMenu]?.focus();
    setOpenMenu(null);
  }

  function closeHelp() {
    setHelp(null);
    triggerRefs.current[3]?.focus();
  }

  useWindowEscapeLayer(openMenu !== null, () => closeMenu());
  useWindowEscapeLayer(help !== null, closeHelp);

  // Global navigation can also arrive while the same destination is already open.
  useEffect(() => {
    setOpenMenu(null);
    setHelp(null);
  }, [props.dismissRequest]);

  useEffect(() => {
    if (openMenu === null) return;
    const onOutsidePointer = (event: PointerEvent) => {
      if (event.target instanceof Node && !navigationRef.current?.contains(event.target)) closeMenu(false);
    };
    window.addEventListener("pointerdown", onOutsidePointer);
    return () => window.removeEventListener("pointerdown", onOutsidePointer);
  }, [openMenu]);

  function menuButtons() {
    return Array.from(menuRef.current?.querySelectorAll<HTMLButtonElement>("button:not(:disabled)") ?? []);
  }

  function focusMenu(edge: "first" | "last") {
    const buttons = menuButtons();
    buttons[edge === "first" ? 0 : buttons.length - 1]?.focus();
  }

  useLayoutEffect(() => {
    if (openMenu !== null && pendingFocus.current) focusMenu(pendingFocus.current);
    pendingFocus.current = null;
  }, [openMenu]);

  useLayoutEffect(() => {
    if (help !== null) helpCloseRef.current?.focus();
  }, [help]);

  function open(index: number, focus: "first" | "last" | null = null) {
    setActiveTrigger(index);
    if (openMenu === index && focus) focusMenu(focus);
    else pendingFocus.current = focus;
    setOpenMenu(index);
  }

  function run(command: MenuCommand) {
    if (command.disabled) return;
    closeMenu();
    setHelp(null);
    try {
      Promise.resolve(command.action()).catch((error: unknown) => onError?.(error instanceof Error ? error.message : String(error)));
    } catch (error) {
      onError?.(error instanceof Error ? error.message : String(error));
    }
  }

  const menus: MenuCommand[][] = [
    [
      { label: zh ? "新建项目" : "New project", action: onNewProject },
      { label: zh ? "新会话" : "New conversation", action: () => onNewConversation?.(), disabled: !hasProject || !onNewConversation || newConversationDisabled },
      { label: zh ? "项目列表" : "Projects", action: onProjects, separatorBefore: true },
      { label: zh ? "设置" : "Settings", action: () => onSettings("general") },
    ],
    [
      { label: zh ? "搜索" : "Search", action: onSearch, shortcut: "Ctrl+K" },
      { label: zh ? "模型设置" : "Models", action: () => onSettings("models"), separatorBefore: true },
      { label: "Skills", action: () => onSettings("skills") },
      { label: zh ? "MCP 连接" : "MCP connections", action: () => onSettings("connections") },
    ],
    [
      { label: zh ? "项目文件" : "Project files", action: onFiles, disabled: !hasProject },
      { label: zh ? "外观设置" : "Appearance", action: () => onSettings("appearance") },
      { label: zh ? "浅色" : "Light", action: () => setTheme("light"), theme: "light", separatorBefore: true },
      { label: zh ? "深色" : "Dark", action: () => setTheme("dark"), theme: "dark" },
      { label: zh ? "跟随系统" : "System", action: () => setTheme("system"), theme: "system" },
    ],
    [
      { label: zh ? "快速入门" : "Quick start", action: () => setHelp("quick-start") },
      { label: zh ? "关于 OmicsOps" : "About OmicsOps", action: () => setHelp("about"), separatorBefore: true },
    ],
  ];

  function onTriggerKeyDown(event: KeyboardEvent<HTMLButtonElement>, index: number) {
    if (["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) {
      event.preventDefault();
      const next = event.key === "Home" ? 0 : event.key === "End" ? 3 : (index + (event.key === "ArrowRight" ? 1 : 3)) % 4;
      setActiveTrigger(next);
      triggerRefs.current[next]?.focus();
      if (openMenu !== null) open(next);
    } else if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      open(index, event.key === "ArrowDown" ? "first" : "last");
    }
  }

  function onMenuKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (openMenu === null) return;
    if (["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) {
      event.preventDefault();
      event.stopPropagation();
      const buttons = menuButtons();
      const current = buttons.indexOf(document.activeElement as HTMLButtonElement);
      const next = event.key === "Home" ? 0 : event.key === "End" ? buttons.length - 1 : (current + (event.key === "ArrowDown" ? 1 : buttons.length - 1)) % buttons.length;
      buttons[next]?.focus();
    } else if (event.key === "ArrowLeft" || event.key === "ArrowRight") {
      event.preventDefault();
      event.stopPropagation();
      open((openMenu + (event.key === "ArrowRight" ? 1 : 3)) % 4, "first");
    }
  }

  const helpTitle = help === "quick-start" ? (zh ? "快速入门" : "Quick start") : (zh ? "关于 OmicsOps" : "About OmicsOps");

  return <>
    <header className="desktop-menu-bar" onKeyDown={(event) => {
      if (event.key === "Tab" && openMenu !== null) closeMenu();
    }}>
      <div className="desktop-menu-brand" data-tauri-drag-region="deep">
        <Dna size={17} aria-hidden="true" />
        <span>OmicsOps</span>
        <small>v{version}</small>
      </div>
      <nav ref={navigationRef} role="menubar" aria-label={zh ? "应用菜单" : "Application menu"} className="desktop-menu-navigation">
        {MENU_NAMES.map((name, index) => <div className="desktop-menu-anchor" role="none" key={name}>
          <button
            type="button"
            role="menuitem"
            id={`${id}-trigger-${index}`}
            aria-haspopup="menu"
            aria-expanded={openMenu === index}
            aria-controls={openMenu === index ? `${id}-menu-${index}` : undefined}
            className="desktop-menu-trigger"
            tabIndex={activeTrigger === index ? 0 : -1}
            ref={(element) => { triggerRefs.current[index] = element; }}
            onFocus={() => setActiveTrigger(index)}
            onClick={() => openMenu === index ? closeMenu() : open(index)}
            onPointerEnter={() => { if (openMenu !== null && openMenu !== index) open(index); }}
            onKeyDown={(event) => onTriggerKeyDown(event, index)}
          >{name}</button>
          {openMenu === index && <div
            role="menu"
            id={`${id}-menu-${index}`}
            aria-labelledby={`${id}-trigger-${index}`}
            className="desktop-menu-popup"
            ref={menuRef}
            onKeyDown={onMenuKeyDown}
          >
            {menus[index].map((command) => <div role="none" key={command.label}>
              {command.separatorBefore && <div role="separator" className="desktop-menu-separator" />}
              <button
                type="button"
                role={command.theme ? "menuitemradio" : "menuitem"}
                aria-checked={command.theme ? theme === command.theme : undefined}
                className="desktop-menu-command"
                disabled={command.disabled}
                tabIndex={-1}
                onClick={() => run(command)}
              >
                <span className="desktop-menu-check" aria-hidden="true">{command.theme === theme ? "✓" : ""}</span>
                <span>{command.label}</span>
                {command.shortcut && <kbd aria-hidden="true">{command.shortcut}</kbd>}
              </button>
            </div>)}
          </div>}
        </div>)}
      </nav>
      <div className="desktop-menu-drag-space" data-tauri-drag-region />
      {children}
    </header>
    {help !== null && <div className="desktop-help-backdrop" onClick={(event) => { if (event.target === event.currentTarget) closeHelp(); }}>
      <section className="desktop-help-dialog" role="dialog" aria-modal="true" aria-labelledby={`${id}-help-title`} onKeyDown={(event) => {
        if (event.key === "Tab") {
          event.preventDefault();
          helpCloseRef.current?.focus();
        }
      }}>
        <header>
          <h2 id={`${id}-help-title`}>{helpTitle}</h2>
          <button type="button" ref={helpCloseRef} onClick={closeHelp} aria-label={zh ? "关闭" : "Close"}><X size={18} aria-hidden="true" /></button>
        </header>
        {help === "quick-start" ? <ol>
          <li>{zh ? "在 File 菜单中新建项目，选择工作目录。" : "Create a project from File and choose its working directory."}</li>
          <li>{zh ? "在 Edit 菜单中配置模型、Skills 和 MCP 连接。" : "Configure a model, Skills and MCP connections from Edit."}</li>
          <li>{zh ? "选择本地或 SSH 执行环境，在会话中描述研究目标。" : "Choose a local or SSH execution environment and describe your research goal in a conversation."}</li>
          <li>{zh ? "检查执行计划与审批，运行后查看产物和原始工具证据。" : "Review the plan and approvals, then inspect artifacts and original tool evidence after execution."}</li>
        </ol> : <div className="desktop-help-about">
          <Dna size={30} aria-hidden="true" />
          <strong>OmicsOps <small>v{version}</small></strong>
          <p>{zh ? "面向科研工作的生物信息学桌面工作台。" : "A bioinformatics desktop workspace for research."}</p>
          <p>{zh ? "在项目中连接对话、计算、文献与可核验的证据，支持本地和 SSH 执行环境。" : "Connect conversations, computation, literature and verifiable evidence in a project, with local and SSH execution environments."}</p>
        </div>}
      </section>
    </div>}
  </>;
}
