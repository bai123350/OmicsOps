import { useEffect, useState } from "react";
import { X } from "lucide-react";

import { usePetPreferences } from "../../use-pet-preferences";
import type { Locale } from "./copy";
import { useWindowEscapeLayer } from "../settings/BrowserSettings";

export type PetCompanionStatus = "idle" | "running" | "needs_attention";

export interface PetCompanionProps {
  status?: PetCompanionStatus;
  onOpenCurrentRun?: () => void;
  locale?: Locale;
  preview?: boolean;
}

function useReducedMotion(): boolean {
  const [reduced, setReduced] = useState(() => (
    typeof window !== "undefined"
    && typeof window.matchMedia === "function"
    && window.matchMedia("(prefers-reduced-motion: reduce)").matches
  ));

  useEffect(() => {
    if (typeof window === "undefined" || typeof window.matchMedia !== "function") return;
    const media = window.matchMedia("(prefers-reduced-motion: reduce)");
    setReduced(media.matches);
    const handleChange = (event: MediaQueryListEvent) => setReduced(event.matches);
    media.addEventListener("change", handleChange);
    return () => media.removeEventListener("change", handleChange);
  }, []);

  return reduced;
}

function PetArtwork({ style }: { style: "orb" | "cell" }) {
  return (
    <svg className="pet-artwork" viewBox="0 0 96 96" aria-hidden="true">
      <defs>
        <linearGradient id={`pet-body-${style}`} x1="18" y1="12" x2="78" y2="84" gradientUnits="userSpaceOnUse">
          <stop stopColor={style === "orb" ? "#8AA8FF" : "#76D8C0"} />
          <stop offset="1" stopColor={style === "orb" ? "#425FD1" : "#218B7A"} />
        </linearGradient>
      </defs>
      {style === "cell" && <>
        <circle className="pet-satellite pet-satellite-one" cx="19" cy="30" r="6" />
        <circle className="pet-satellite pet-satellite-two" cx="78" cy="20" r="4" />
        <circle className="pet-satellite pet-satellite-three" cx="81" cy="68" r="7" />
      </>}
      <path
        className="pet-body"
        d={style === "orb"
          ? "M48 11c23 0 38 16 38 38S71 86 48 86 10 72 10 49 25 11 48 11Z"
          : "M48 12c20 0 35 12 38 31 4 22-10 41-33 44C31 90 13 77 10 56 7 33 24 12 48 12Z"}
        fill={`url(#pet-body-${style})`}
      />
      <ellipse cx="34" cy="47" rx="4" ry="5" fill="#102047" />
      <ellipse cx="62" cy="47" rx="4" ry="5" fill="#102047" />
      <circle cx="35" cy="45" r="1.4" fill="white" />
      <circle cx="63" cy="45" r="1.4" fill="white" />
      <path d="M40 60c5 5 11 5 16 0" fill="none" stroke="white" strokeLinecap="round" strokeWidth="3" />
      <path className="pet-spark" d="m72 33 2 5 5 2-5 2-2 5-2-5-5-2 5-2 2-5Z" fill="#FFF2A8" />
    </svg>
  );
}

function statusLabel(status: PetCompanionStatus, zh: boolean): string {
  if (status === "running") return zh ? "运行中" : "Running";
  if (status === "needs_attention") return zh ? "需要关注" : "Needs attention";
  return zh ? "空闲" : "Idle";
}

function VisiblePetCompanion({
  status,
  onOpenCurrentRun,
  locale = "en-US",
  preview = false,
}: PetCompanionProps) {
  const { name, style, size, setEnabled } = usePetPreferences();
  const [detailsOpen, setDetailsOpen] = useState(false);
  const reducedMotion = useReducedMotion();
  const zh = locale === "zh-CN";
  const displayName = name.trim() || (zh ? "研究伴侣" : "Companion");

  useWindowEscapeLayer(detailsOpen, () => setDetailsOpen(false));

  if (preview) {
    return (
      <section
        className="pet-companion pet-companion-preview"
        data-style={style}
        data-size={size}
        data-motion={reducedMotion ? "reduced" : "full"}
        aria-label={zh ? "伴侣预览" : "Pet preview"}
      >
        <PetArtwork style={style} />
        <strong>{displayName}</strong>
      </section>
    );
  }

  return (
    <aside
      className="pet-companion pet-companion-floating"
      data-style={style}
      data-size={size}
      data-motion={reducedMotion ? "reduced" : "full"}
      aria-label={zh ? `${displayName} 研究伴侣` : `${displayName} research companion`}
    >
      {detailsOpen && (
        <section className="pet-details" role="dialog" aria-modal="false" aria-label={zh ? `${displayName} 详情` : `${displayName} details`}>
          <header>
            <strong>{displayName}</strong>
            <button type="button" onClick={() => setDetailsOpen(false)} aria-label={zh ? "关闭伴侣详情" : "Close companion details"}>
              <X size={14} aria-hidden="true" />
            </button>
          </header>
          {status ? (
            <p className={`pet-status pet-status-${status}`}>{statusLabel(status, zh)}</p>
          ) : (
            <p>{zh ? "窗口内研究伴侣，不会生成科研建议。" : "A window companion that does not generate research advice."}</p>
          )}
          {status && onOpenCurrentRun && (
            <button type="button" className="pet-run-action" onClick={onOpenCurrentRun}>
              {zh ? "查看当前运行" : "View current run"}
            </button>
          )}
        </section>
      )}
      <button
        type="button"
        className="pet-open"
        onClick={() => setDetailsOpen((open) => !open)}
        aria-expanded={detailsOpen}
        aria-label={zh ? `打开 ${displayName} 伴侣` : `Open ${displayName} companion`}
      >
        <PetArtwork style={style} />
      </button>
      <button type="button" className="pet-hide" onClick={() => setEnabled(false)} aria-label={zh ? `隐藏 ${displayName}` : `Hide ${displayName}`}>
        <X size={12} aria-hidden="true" />
      </button>
    </aside>
  );
}

export function PetCompanion(props: PetCompanionProps) {
  const { enabled } = usePetPreferences();
  if (!enabled && !props.preview) return null;
  return <VisiblePetCompanion {...props} />;
}
