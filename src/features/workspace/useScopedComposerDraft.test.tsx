import { act, renderHook } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { ComposerDraftStore, type DraftScope } from "./composerDrafts";
import { useScopedComposerDraft } from "./useScopedComposerDraft";

const a: DraftScope = { projectId: "p", conversationId: "a" };
const b: DraftScope = { projectId: "p", conversationId: "b" };

describe("useScopedComposerDraft", () => {
  it("immediately reads the new scope on rerender and restores the old one", () => {
    const store = new ComposerDraftStore();
    store.update(a, "A");
    store.update(b, "B");
    const { result, rerender } = renderHook(({ scope }) => useScopedComposerDraft(store, scope),
      { initialProps: { scope: a } });
    expect(result.current.draft).toBe("A");
    rerender({ scope: b });
    expect(result.current.draft).toBe("B");
    act(() => result.current.setDraft((text) => text + " edited"));
    rerender({ scope: a });
    expect(result.current.draft).toBe("A");
    expect(store.read(b).text).toBe("B edited");
  });

  it("binds delayed setters and capture to their render scope", () => {
    const store = new ComposerDraftStore();
    const { result, rerender } = renderHook(({ scope }) => useScopedComposerDraft(store, scope),
      { initialProps: { scope: a } });
    const oldSet = result.current.setDraft;
    const oldCapture = result.current.capture;
    rerender({ scope: b });
    act(() => result.current.setDraft("B"));
    act(() => oldSet("A later"));
    expect(result.current.draft).toBe("B");
    expect(oldCapture()).toEqual({ scope: a, text: "A later", revision: 1 });
    expect(result.current.capture()).toEqual({ scope: b, text: "B", revision: 1 });
  });

  it("observes external changes and accepted clear without clearing newer edits", () => {
    const store = new ComposerDraftStore();
    const { result } = renderHook(() => useScopedComposerDraft(store, a));
    act(() => store.update(a, "sent"));
    expect(result.current.draft).toBe("sent");
    const old = result.current.capture();
    act(() => result.current.setDraft("new"));
    act(() => store.clearAccepted(old));
    expect(result.current.draft).toBe("new");
    act(() => store.clearAccepted(result.current.capture()));
    expect(result.current.draft).toBe("");
  });

  it("retains drafts through unmount with the same app store", () => {
    const store = new ComposerDraftStore();
    const first = renderHook(() => useScopedComposerDraft(store, a));
    act(() => first.result.current.setDraft("retained"));
    first.unmount();
    const second = renderHook(() => useScopedComposerDraft(store, a));
    expect(second.result.current.draft).toBe("retained");
    const fresh = renderHook(() => useScopedComposerDraft(new ComposerDraftStore(), a));
    expect(fresh.result.current.draft).toBe("");
  });

  it("switches injected stores and keeps old callbacks on the original store", () => {
    const first = new ComposerDraftStore();
    const second = new ComposerDraftStore();
    second.update(a, "second");
    const { result, rerender } = renderHook(({ store }) => useScopedComposerDraft(store, a),
      { initialProps: { store: first } });
    const oldSet = result.current.setDraft;
    rerender({ store: second });
    expect(result.current.draft).toBe("second");
    act(() => oldSet("first"));
    expect(result.current.draft).toBe("second");
    expect(first.read(a).text).toBe("first");
  });
});
