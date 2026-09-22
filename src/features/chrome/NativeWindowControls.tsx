import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Copy, Minus, Square, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { Locale } from "../workspace/copy";
import "./native-window-controls.css";

type WindowAction = "minimize" | "toggleMaximize" | "close";

export function NativeWindowControls({ locale, onError }: { locale: Locale; onError?: (message: string) => void }) {
  const enabled = isTauri() && /Windows/i.test(navigator.userAgent);
  const [maximized, setMaximized] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const props = useRef({ locale, onError });
  props.current = { locale, onError };
  const execute = useRef<((action: WindowAction) => Promise<void>) | null>(null);

  useEffect(() => {
    if (!enabled) return;
    let active = true;
    let unlisten: (() => void) | undefined;
    let query = 0;
    const reportError = () => {
      if (!active) return;
      const message = props.current.locale === "zh-CN" ? "无法更新窗口状态。" : "Could not update the window.";
      setError(message);
      props.current.onError?.(message);
    };

    try {
      const appWindow = getCurrentWindow();
      const refreshMaximized = async () => {
        const currentQuery = ++query;
        try {
          const next = await appWindow.isMaximized();
          if (active && currentQuery === query) setMaximized(next);
        } catch {
          if (currentQuery === query) reportError();
        }
      };
      execute.current = async (action) => {
        try {
          await appWindow[action]();
          if (!active) return;
          setError(null);
          if (action === "toggleMaximize") await refreshMaximized();
        } catch {
          reportError();
        }
      };
      void refreshMaximized();
      void appWindow.onResized(() => { void refreshMaximized(); }).then((stop) => {
        if (active) unlisten = stop;
        else stop();
      }).catch(reportError);
    } catch {
      reportError();
    }

    return () => {
      active = false;
      execute.current = null;
      unlisten?.();
    };
  }, [enabled]);

  if (!enabled) return null;
  const zh = locale === "zh-CN";
  const maximizeLabel = maximized
    ? (zh ? "还原窗口" : "Restore window")
    : (zh ? "最大化窗口" : "Maximize window");

  return <div className="native-window-controls" role="group" aria-label={zh ? "窗口控制" : "Window controls"}>
    <button type="button" aria-label={zh ? "最小化窗口" : "Minimize window"} title={zh ? "最小化窗口" : "Minimize window"} onClick={() => { void execute.current?.("minimize"); }}><Minus aria-hidden="true" /></button>
    <button type="button" aria-label={maximizeLabel} title={maximizeLabel} onClick={() => { void execute.current?.("toggleMaximize"); }}>{maximized ? <Copy aria-hidden="true" /> : <Square aria-hidden="true" />}</button>
    <button className="native-window-close" type="button" aria-label={zh ? "关闭窗口" : "Close window"} title={zh ? "关闭窗口" : "Close window"} onClick={() => { void execute.current?.("close"); }}><X aria-hidden="true" /></button>
    {error && <span className="native-window-error" role="status">{error}</span>}
  </div>;
}
