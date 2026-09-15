import type { Locale } from "../workspace/copy";
import {
  useAppearance,
  type AppearanceCodeFont,
  type AppearanceTheme,
  type AppearanceUiFont,
  type AppearanceUiScale,
} from "../../use-appearance";

export function AppearanceSettings({ locale }: { locale: Locale }) {
  const zh = locale === "zh-CN";
  const {
    theme,
    uiFont,
    codeFont,
    uiScale,
    setTheme,
    setUiFont,
    setCodeFont,
    setUiScale,
  } = useAppearance();

  return (
    <main className="appearance-settings">
      <header className="appearance-heading">
        <h3>{zh ? "外观" : "Appearance"}</h3>
        <p>{zh ? "调整 OmicsOps 在这台设备上的显示方式。更改会立即生效。" : "Adjust how OmicsOps looks on this device. Changes apply immediately."}</p>
      </header>

      <section className="appearance-card" aria-labelledby="appearance-color-heading">
        <div className="appearance-group-heading">
          <h4 id="appearance-color-heading">{zh ? "颜色" : "Color"}</h4>
        </div>
        <label className="appearance-row">
          <span className="appearance-copy">
            <strong>{zh ? "主题" : "Theme"}</strong>
            <small>{zh ? "跟随系统，或始终使用浅色或深色界面。" : "Follow the system, or always use a light or dark interface."}</small>
          </span>
          <select aria-label={zh ? "主题" : "Theme"} value={theme} onChange={(event) => setTheme(event.target.value as AppearanceTheme)}>
            <option value="system">{zh ? "跟随系统" : "System"}</option>
            <option value="light">{zh ? "浅色" : "Light"}</option>
            <option value="dark">{zh ? "深色" : "Dark"}</option>
          </select>
        </label>
      </section>

      <section className="appearance-card" aria-labelledby="appearance-type-heading">
        <div className="appearance-group-heading">
          <h4 id="appearance-type-heading">{zh ? "字体" : "Typography"}</h4>
          <p>{zh ? "只提供内置字体组合，不会加载外部字体或自定义 CSS。" : "Only built-in font stacks are available; no external fonts or custom CSS are loaded."}</p>
        </div>
        <label className="appearance-row">
          <span className="appearance-copy">
            <strong>{zh ? "界面字体" : "Interface font"}</strong>
            <small>{zh ? "用于导航、正文、表单和对话框。" : "Used for navigation, prose, forms, and dialogs."}</small>
          </span>
          <select aria-label={zh ? "界面字体" : "Interface font"} value={uiFont} onChange={(event) => setUiFont(event.target.value as AppearanceUiFont)}>
            <option value="system">{zh ? "系统默认" : "System default"}</option>
            <option value="sans">{zh ? "无衬线" : "Sans serif"}</option>
          </select>
        </label>
        <label className="appearance-row">
          <span className="appearance-copy">
            <strong>{zh ? "代码字体" : "Code font"}</strong>
            <small>{zh ? "用于代码块、命令、路径和技术标识。" : "Used for code blocks, commands, paths, and technical identifiers."}</small>
          </span>
          <select aria-label={zh ? "代码字体" : "Code font"} value={codeFont} onChange={(event) => setCodeFont(event.target.value as AppearanceCodeFont)}>
            <option value="system">{zh ? "系统等宽" : "System monospace"}</option>
            <option value="mono">{zh ? "Cascadia 等宽" : "Cascadia mono"}</option>
          </select>
        </label>
      </section>

      <section className="appearance-card" aria-labelledby="appearance-scale-heading">
        <div className="appearance-group-heading">
          <h4 id="appearance-scale-heading">{zh ? "界面缩放" : "Interface scale"}</h4>
          <p>{zh ? "同时缩放导航、正文、输入框和对话框，而不只是文字。" : "Scales navigation, prose, inputs, and dialogs together, rather than text alone."}</p>
        </div>
        <label className="appearance-row">
          <span className="appearance-copy">
            <strong>{zh ? "缩放级别" : "Scale level"}</strong>
            <small>{zh ? `${Math.round(uiScale * 100)}% — ${scaleName(uiScale, true)}` : `${Math.round(uiScale * 100)}% — ${scaleName(uiScale, false)}`}</small>
          </span>
          <select
            aria-label={zh ? "界面缩放" : "Interface scale"}
            value={String(uiScale)}
            onChange={(event) => setUiScale(Number(event.target.value) as AppearanceUiScale)}
          >
            <option value="0.9">90%</option>
            <option value="1">100%</option>
            <option value="1.1">110%</option>
            <option value="1.2">120%</option>
          </select>
        </label>
      </section>
    </main>
  );
}

function scaleName(scale: AppearanceUiScale, zh: boolean): string {
  if (scale === 0.9) return zh ? "紧凑" : "Compact";
  if (scale === 1) return zh ? "默认" : "Default";
  if (scale === 1.1) return zh ? "较大" : "Larger";
  return zh ? "最大" : "Largest";
}
