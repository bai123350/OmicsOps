import { describe, expect, it, vi } from "vitest";
import { ComposerDraftStore } from "./composerDrafts";

const a = { projectId: "p", conversationId: "a" };
const b = { projectId: "p", conversationId: "b" };
const unassigned = { projectId: "p", conversationId: null };

describe("ComposerDraftStore", () => {
  it("returns stable immutable empty and written snapshots", () => {
    const store = new ComposerDraftStore();
    const empty = store.read(a);
    expect(empty).toEqual({ text: "", revision: 0 });
    expect(store.read({ ...a })).toBe(empty);
    expect(Object.isFrozen(empty)).toBe(true);
    const written = store.update(a, "draft");
    expect(store.read(a)).toBe(written);
    expect(Object.isFrozen(written)).toBe(true);
    expect(empty.text).toBe("");
  });

  it("isolates conversations, projects, nullable IDs, tuple keys, and store instances", () => {
    const store = new ComposerDraftStore();
    const scopes = [a, b, unassigned, { projectId: "q", conversationId: "a" },
      { projectId: "p", conversationId: "null" },
      { projectId: "p:a", conversationId: "b" },
      { projectId: "p", conversationId: "a:b" }];
    scopes.forEach((scope, index) => store.update(scope, String(index)));
    scopes.forEach((scope, index) => expect(store.read(scope).text).toBe(String(index)));
    expect(new ComposerDraftStore().read(a).text).toBe("");
  });

  it("notifies only the changed scope and honors unsubscribe", () => {
    const store = new ComposerDraftStore();
    const onA = vi.fn();
    const onB = vi.fn();
    const unsubscribe = store.subscribe(a, onA);
    store.subscribe(b, onB);
    store.update({ ...a }, "one");
    store.update(a, "one");
    expect(onA).toHaveBeenCalledTimes(2);
    expect(onB).not.toHaveBeenCalled();
    unsubscribe();
    store.update(a, "two");
    expect(onA).toHaveBeenCalledTimes(2);
  });

  it("applies functional edits to the latest text and advances every write", () => {
    const store = new ComposerDraftStore();
    store.update(a, "x");
    expect(store.update(a, (text) => text + "y")).toEqual({ text: "xy", revision: 2 });
    expect(store.update(a, "xy").revision).toBe(3);
  });

  it("does not let a repeated old unsubscribe remove a newer subscription", () => {
    const store = new ComposerDraftStore();
    const unsubscribe = store.subscribe(a, () => {});
    unsubscribe();
    const listener = vi.fn();
    store.subscribe(a, listener);
    unsubscribe();
    store.update(a, "new");
    expect(listener).toHaveBeenCalledTimes(1);
  });

  it("captures immutable scope and text without clearing on capture", () => {
    const store = new ComposerDraftStore();
    const scope = { ...a };
    store.update(scope, "pending");
    const receipt = store.capture(scope);
    scope.conversationId = "b";
    expect(receipt).toEqual({ scope: a, text: "pending", revision: 1 });
    expect(Object.isFrozen(receipt)).toBe(true);
    expect(Object.isFrozen(receipt.scope)).toBe(true);
    expect(store.read(a).text).toBe("pending");
  });

  it("clears an accepted offscreen receipt once while preserving other scopes", () => {
    const store = new ComposerDraftStore();
    store.update(a, "sent");
    const receipt = store.capture(a);
    store.update(b, "current");
    expect(store.clearAccepted(receipt)).toBe(true);
    expect(store.read(a)).toEqual({ text: "", revision: 2 });
    expect(store.clearAccepted(receipt)).toBe(false);
    expect(store.read(b).text).toBe("current");
  });

  it("keeps edited same-text drafts when the old receipt arrives", () => {
    const store = new ComposerDraftStore();
    store.update(a, "same");
    const receipt = store.capture(a);
    store.update(a, "");
    store.update(a, "same");
    expect(store.clearAccepted(receipt)).toBe(false);
    expect(store.read(a).text).toBe("same");
    expect(store.read(a)).toBe(store.read(a));
  });

  it("adopts only existing unassigned text into an empty target", () => {
    const store = new ComposerDraftStore();
    expect(store.adoptUnassigned("p", "a")).toBe(false);
    store.update(unassigned, "new conversation");
    const receipt = store.capture(unassigned);
    const onSource = vi.fn();
    const onTarget = vi.fn();
    store.subscribe(unassigned, onSource);
    store.subscribe(a, onTarget);
    expect(store.adoptUnassigned("p", "a")).toBe(true);
    expect(store.read(a)).toEqual({ text: "new conversation", revision: 1 });
    expect(store.read(unassigned)).toEqual({ text: "", revision: 2 });
    expect(store.clearAccepted(receipt)).toBe(false);
    expect(onSource).toHaveBeenCalledTimes(1);
    expect(onTarget).toHaveBeenCalledTimes(1);
  });

  it("does not overwrite a target or discard the unassigned draft", () => {
    const store = new ComposerDraftStore();
    store.update(unassigned, "source");
    store.update(a, "target");
    const source = store.read(unassigned);
    const target = store.read(a);
    expect(store.adoptUnassigned("p", "a")).toBe(false);
    expect(store.read(unassigned)).toBe(source);
    expect(store.read(a)).toBe(target);
  });

  it("advances empty target versions during adoption to reject old receipts", () => {
    const store = new ComposerDraftStore();
    const old = store.capture(a);
    store.update(unassigned, "source");
    store.adoptUnassigned("p", "a");
    expect(store.clearAccepted(old)).toBe(false);
    expect(store.read(a).text).toBe("source");
  });

  it("deletes conversation text and retains revisions against receipt reuse", () => {
    const store = new ComposerDraftStore();
    store.update(a, "same");
    const old = store.capture(a);
    const listener = vi.fn();
    store.subscribe(a, listener);
    store.removeConversation(a);
    expect(store.read(a)).toEqual({ text: "", revision: 2 });
    store.removeConversation(a);
    expect(store.read(a).revision).toBe(3);
    store.update(a, "same");
    expect(store.clearAccepted(old)).toBe(false);
    expect(listener).toHaveBeenCalledTimes(3);
  });

  it("deletes all project scopes including null and preserves other projects", () => {
    const store = new ComposerDraftStore();
    const other = { projectId: "other", conversationId: "a" };
    for (const scope of [a, b, unassigned, other]) store.update(scope, "draft");
    const old = store.capture(a);
    const listener = vi.fn();
    store.subscribe(a, listener);
    store.removeProject("p");
    for (const scope of [a, b, unassigned]) {
      expect(store.read(scope)).toEqual({ text: "", revision: 2 });
    }
    expect(store.read(other).text).toBe("draft");
    store.removeProject("p");
    expect(store.read(a).revision).toBe(3);
    expect(store.clearAccepted(old)).toBe(false);
    expect(listener).toHaveBeenCalledTimes(2);
  });
});
