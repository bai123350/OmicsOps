import { expect, it } from "vitest";
import { messageLibrarySource } from "./librarySources";

it("hashes the whole persisted message and uses exact UTF-8 excerpt offsets", async () => {
  const text = "前言\nα😀 code\n结尾";
  const source = await messageLibrarySource("p", "c", "m", text, 3, 11);
  expect(source.start).toBe(new TextEncoder().encode(text.slice(0, 3)).length);
  expect(source.end).toBe(new TextEncoder().encode(text.slice(0, 11)).length);
  expect(source.content_sha256).toMatch(/^[a-f0-9]{64}$/);
  expect(source).toMatchObject({ kind: "message", id: "m", project_id: "p", conversation_id: "c" });
});
