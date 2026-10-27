import { createContext, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import ReactMarkdown, { defaultUrlTransform, type Components } from "react-markdown";
import remarkGfm from "remark-gfm";
import type { ProjectImagePreview } from "../../types";
import { isSidebarPreviewImage } from "./sidebarData";

type ImageLoader = (path: string) => Promise<ProjectImagePreview>;
type ImageScope = { remoteRoot?: string | null; load?: ImageLoader };
const ImageContext = createContext<{ scope: ImageScope; zh: boolean }>({ scope: {}, zh: false });

export function ProjectMarkdownImages({ scopeKey, remoteRoot, onPreviewImage, zh, children }: {
  scopeKey: string; remoteRoot?: string | null; onPreviewImage?: ImageLoader; zh: boolean; children: ReactNode;
}) {
  // DesktopApp supplies an inline callback. Rendering new tokens must not start
  // new SSH connections; project/conversation/binding changes must invalidate it.
  const latestLoader = useRef(onPreviewImage);
  latestLoader.current = onPreviewImage;
  const enabled = Boolean(onPreviewImage);
  const scope = useMemo<ImageScope>(() => {
    const pending = new Map<string, Promise<ProjectImagePreview>>();
    return { remoteRoot, load: enabled ? (path) => {
      const existing = pending.get(path);
      if (existing) return existing;
      const loader = latestLoader.current;
      const request = Promise.resolve().then(() => {
        if (!loader) throw new Error("Image preview unavailable");
        return loader(path);
      }).finally(() => { pending.delete(path); });
      pending.set(path, request);
      return request;
    } : undefined };
  }, [scopeKey, remoteRoot, enabled]);
  return <ImageContext.Provider value={{ scope, zh }}>{children}</ImageContext.Provider>;
}

function projectImagePath(source: string, remoteRoot?: string | null): string | null {
  let path = source.split(/[?#]/, 1)[0];
  if (path.startsWith("file:///")) path = path.slice("file://".length);
  try { path = decodeURIComponent(path); } catch { return null; }
  if (!path || /[\x00-\x1f\x7f\\:]/.test(path) || path.startsWith("//")) return null;
  // Reject traversal before removing the root or normalizing ./ segments.
  if (path.split("/").includes("..")) return null;
  if (path.startsWith("/")) {
    const root = remoteRoot?.replace(/\/+$/, "");
    if (root === undefined || root === null || !remoteRoot?.startsWith("/") || !path.startsWith(`${root}/`)) return null;
    path = path.slice(root.length + 1);
  }
  const parts = path.split("/").filter((part) => part !== ".");
  if (parts.some((part) => !part || part.toLowerCase() === ".omicsops")) return null;
  path = parts.join("/");
  return isSidebarPreviewImage(path) ? path : null;
}

type ImageState = { scope: ImageScope; path: string; dataUrl?: string; failed?: boolean };
function MarkdownImage({ src, alt, title }: { src?: string | Blob; alt?: string; title?: string }) {
  const { scope, zh } = useContext(ImageContext);
  const source = typeof src === "string" ? src : "";
  const external = /^(https?:)?\/\//i.test(source);
  const path = external ? null : projectImagePath(source, scope.remoteRoot);
  const [state, setState] = useState<ImageState | null>(null);
  const [retry, setRetry] = useState(0);
  const current = state?.scope === scope && state.path === path ? state : null;
  useEffect(() => {
    if (!path || !scope.load) return;
    let active = true;
    setState({ scope, path });
    void scope.load(path).then(
      (preview) => { if (active) setState({ scope, path, dataUrl: preview.data_url }); },
      () => { if (active) setState({ scope, path, failed: true }); },
    );
    return () => { active = false; };
  }, [scope, path, retry]);

  if (external) return <img src={source} alt={alt} title={title} />;
  if (!path || !scope.load) return <span className="markdown-image-status">{alt && `${alt} · `}{zh ? "图片不可用：需要受支持的项目图片路径及 SSH 连接。" : "Image unavailable: a supported project image path and SSH connection are required."}</span>;
  if (current?.dataUrl) return <img className="project-markdown-image" src={current.dataUrl} alt={alt} title={title} onError={() => setState({ scope, path, failed: true })} />;
  return <span className="markdown-image-status" role="status">
    {alt && `${alt} · `}{current?.failed ? (zh ? "图片加载失败，请检查连接、文件及预览限制后重试。" : "Could not load image. Check the connection, file, and preview limits, then retry.") : (zh ? "正在加载图片…" : "Loading image…")}
    {current?.failed && <button type="button" onClick={() => setRetry((value) => value + 1)}>{zh ? "重试图片" : "Retry image"}</button>}
  </span>;
}

// Stable component identities preserve pending image loads during streaming.
const components: Components = {
  img: MarkdownImage,
  table: ({ children }) => <div className="markdown-table-scroll" role="region" aria-label="表格 / Table" tabIndex={0}><table>{children}</table></div>,
};

export function ProjectMarkdown({ markdown }: { markdown: string }) {
  return <ReactMarkdown remarkPlugins={[remarkGfm]} components={components} urlTransform={(url, key) =>
    // File URLs never reach an img element directly. MarkdownImage checks the
    // project root and delegates only a relative path to the host preview API.
    key === "src" && url.startsWith("file:///") ? url : defaultUrlTransform(url)
  }>{markdown}</ReactMarkdown>;
}
