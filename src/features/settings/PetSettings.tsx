import type { Locale } from "../workspace/copy";
import { PetCompanion } from "../workspace/PetCompanion";
import {
  usePetPreferences,
  type PetSize,
  type PetStyle,
} from "../../use-pet-preferences";

export function PetSettings({ locale = "en-US" }: { locale?: Locale }) {
  const zh = locale === "zh-CN";
  const { enabled, name, style, size, setEnabled, setName, setStyle, setSize } = usePetPreferences();

  return (
    <main className="pet-settings">
      <header className="pet-settings-heading">
        <h3>{zh ? "研究伴侣" : "Research companion"}</h3>
        <p>{zh ? "在工作区边缘显示一个轻量的内置伴侣。它不会读取文件或生成科研建议。" : "Show a lightweight built-in companion at the edge of the workspace. It does not read files or generate research advice."}</p>
      </header>

      <div className="pet-settings-layout">
        <section className="pet-settings-card" aria-labelledby="pet-behavior-heading">
          <h4 id="pet-behavior-heading">{zh ? "显示" : "Display"}</h4>
          <label className="pet-settings-row">
            <span>
              <strong>{zh ? "显示研究伴侣" : "Show research companion"}</strong>
              <small>{zh ? "关闭只会隐藏伴侣，不会停止正在运行的任务。" : "Turning this off only hides the companion; it does not stop a run."}</small>
            </span>
            <input type="checkbox" checked={enabled} onChange={(event) => setEnabled(event.target.checked)} aria-label={zh ? "显示研究伴侣" : "Show research companion"} />
          </label>

          <label className="pet-settings-field">
            <span>{zh ? "伴侣名称" : "Companion name"}</span>
            <input
              type="text"
              maxLength={24}
              value={name}
              onChange={(event) => setName(event.target.value)}
              aria-label={zh ? "伴侣名称" : "Companion name"}
            />
          </label>

          <div className="pet-settings-controls">
            <label className="pet-settings-field">
              <span>{zh ? "内置样式" : "Built-in style"}</span>
              <select value={style} onChange={(event) => setStyle(event.target.value as PetStyle)} aria-label={zh ? "内置样式" : "Built-in style"}>
                <option value="orb">{zh ? "轨道精灵" : "Orbit sprite"}</option>
                <option value="cell">{zh ? "细胞精灵" : "Cell sprite"}</option>
              </select>
            </label>
            <label className="pet-settings-field">
              <span>{zh ? "尺寸" : "Size"}</span>
              <select value={size} onChange={(event) => setSize(event.target.value as PetSize)} aria-label={zh ? "尺寸" : "Size"}>
                <option value="small">{zh ? "小" : "Small"}</option>
                <option value="medium">{zh ? "中" : "Medium"}</option>
                <option value="large">{zh ? "大" : "Large"}</option>
              </select>
            </label>
          </div>
        </section>

        <section className="pet-preview-card" aria-labelledby="pet-preview-heading">
          <div>
            <h4 id="pet-preview-heading">{zh ? "实时预览" : "Live preview"}</h4>
            <small>{zh ? "预览使用当前名称、样式和尺寸。" : "The preview uses the current name, style, and size."}</small>
          </div>
          <PetCompanion preview locale={locale} />
        </section>
      </div>
    </main>
  );
}
