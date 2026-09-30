import { useState } from "react";
import type { ReactNode } from "react";
import type { SubscriptionResource } from "../../types";
import { openSubscriptionResource } from "../../tauri-api";

const urls: Record<SubscriptionResource, string> = {
  codex_login: "https://auth.openai.com/codex/device",
  claude_setup: "https://code.claude.com/docs/en/setup",
  go_privacy: "https://opencode.ai/docs/go/#privacy",
};
export function SubscriptionResourceLink({ resource, zh, children }: { resource: SubscriptionResource; zh: boolean; children: ReactNode }) {
  const [failed, setFailed] = useState(false);
  return <span><a href={urls[resource]} onClick={event => { event.preventDefault(); setFailed(false); void openSubscriptionResource(resource).catch(() => setFailed(true)); }}>{children}</a>{failed && <span role="alert">{zh ? "请复制此地址并在浏览器中打开：" : "Copy this address and open it in a browser: "}<code>{urls[resource]}</code></span>}</span>;
}
