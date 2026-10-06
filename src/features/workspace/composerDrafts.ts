export type DraftScope = Readonly<{
  projectId: string;
  conversationId: string | null;
}>;

export type DraftSnapshot = Readonly<{ text: string; revision: number }>;
export type DraftReceipt = DraftSnapshot & Readonly<{ scope: DraftScope }>;

function scopeKey(scope: DraftScope): string {
  return JSON.stringify([scope.projectId, scope.conversationId]);
}

/** Owned by one app window; stores only ephemeral composer text. */
export class ComposerDraftStore {
  private readonly snapshots = new Map<string, DraftSnapshot>();
  private readonly listeners = new Map<string, Set<() => void>>();

  read(scope: DraftScope): DraftSnapshot {
    const key = scopeKey(scope);
    let snapshot = this.snapshots.get(key);
    if (!snapshot) {
      snapshot = Object.freeze({ text: "", revision: 0 });
      this.snapshots.set(key, snapshot);
    }
    return snapshot;
  }

  subscribe(scope: DraftScope, listener: () => void): () => void {
    const key = scopeKey(scope);
    this.read(scope);
    let listeners = this.listeners.get(key);
    if (!listeners) {
      listeners = new Set();
      this.listeners.set(key, listeners);
    }
    listeners.add(listener);
    return () => {
      listeners.delete(listener);
      if (listeners.size === 0 && this.listeners.get(key) === listeners) {
        this.listeners.delete(key);
      }
    };
  }

  update(scope: DraftScope, updater: string | ((text: string) => string)): DraftSnapshot {
    const current = this.read(scope);
    const text = typeof updater === "function" ? updater(current.text) : updater;
    const snapshot = Object.freeze({ text, revision: current.revision + 1 });
    const key = scopeKey(scope);
    this.snapshots.set(key, snapshot);
    for (const listener of [...(this.listeners.get(key) ?? [])]) listener();
    return snapshot;
  }

  capture(scope: DraftScope): DraftReceipt {
    return Object.freeze({ ...this.read(scope), scope: Object.freeze({ ...scope }) });
  }

  clearAccepted(receipt: DraftReceipt): boolean {
    if (this.read(receipt.scope).revision !== receipt.revision) return false;
    this.update(receipt.scope, "");
    return true;
  }

  adoptUnassigned(projectId: string, conversationId: string): boolean {
    const source = { projectId, conversationId: null };
    const target = { projectId, conversationId };
    const receipt = this.capture(source);
    if (!receipt.text || this.read(target).text) return false;
    this.update(target, receipt.text);
    // Target subscribers can edit the source while receiving the transfer.
    this.clearAccepted(receipt);
    return true;
  }

  removeConversation(scope: DraftScope): void {
    // Keep the empty version marker so late receipts cannot match a recreated draft.
    this.update(scope, "");
  }

  removeProject(projectId: string): void {
    for (const key of [...this.snapshots.keys()]) {
      const [storedProjectId, conversationId] = JSON.parse(key) as [string, string | null];
      if (storedProjectId === projectId) this.removeConversation({ projectId, conversationId });
    }
  }
}
