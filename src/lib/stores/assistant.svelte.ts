import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import * as api from "$lib/api/ai";
import type { Block, ChatEvent, Conversation, Proposal, StoredMessage } from "$lib/api/ai";
import { aiContext } from "./aicontext.svelte";
import { toast } from "./toast.svelte";

/** Chat state. All logic (tools, confirmation, persistence) lives in Rust; this only renders events. */
export class AssistantStore {
  conversations = $state<Conversation[]>([]);
  currentId = $state<string | null>(null);
  messages = $state<StoredMessage[]>([]);
  proposals = $state<Proposal[]>([]);
  /** Text streaming in for the turn that is running. */
  streaming = $state("");
  tools = $state<string[]>([]);
  busy = $state(false);
  error = $state<string | null>(null);
  /** Optimistic copy of the message just sent, until the stored one is loaded. */
  pendingUser = $state<string | null>(null);

  private unlisten: UnlistenFn | null = null;

  async start() {
    this.unlisten ??= await listen<ChatEvent>("ai-stream", (e) => this.onEvent(e.payload));
    await this.refreshList();
  }
  stop() {
    this.unlisten?.();
    this.unlisten = null;
  }

  async refreshList() {
    try {
      this.conversations = await api.listConversations();
    } catch (e) {
      toast.error(e);
    }
  }

  async open(id: string | null) {
    this.currentId = id;
    this.error = null;
    this.streaming = "";
    this.tools = [];
    if (!id) {
      this.messages = [];
      this.proposals = [];
      return;
    }
    try {
      const d = await api.getConversation(id);
      this.messages = d.messages;
      this.proposals = d.proposals;
    } catch (e) {
      toast.error(e);
    }
  }

  private onEvent(ev: ChatEvent) {
    if (ev.kind === "started") {
      // Only follow events of the conversation this view started or has open.
      if (this.busy && !this.currentId) this.currentId = ev.conversation_id;
    } else if (ev.kind === "text_delta") {
      this.streaming += ev.text;
    } else if (ev.kind === "tool_started") {
      this.tools = [...this.tools, ev.name];
    } else if (ev.kind === "proposal") {
      this.proposals = [...this.proposals.filter((p) => p.id !== ev.proposal.id), ev.proposal];
    } else if (ev.kind === "error") {
      this.error = ev.message;
    }
  }

  async send(text: string) {
    text = text.trim();
    if (!text || this.busy) return;
    this.busy = true;
    this.error = null;
    this.streaming = "";
    this.tools = [];
    this.pendingUser = text;
    try {
      const id = await api.sendMessage(this.currentId, text, aiContext.toPayload());
      this.currentId = id;
    } catch (e) {
      // The error event already set `error`; keep it if the command failed before streaming.
      this.error ??= e instanceof Error ? e.message : String(e);
    } finally {
      this.pendingUser = null;
      this.busy = false;
      this.streaming = "";
      this.tools = [];
      if (this.currentId) await this.open(this.currentId);
      await this.refreshList();
    }
  }

  async cancel() {
    if (this.currentId) await api.cancelMessage(this.currentId).catch(() => {});
  }

  async remove(id: string) {
    try {
      await api.deleteConversation(id);
      if (this.currentId === id) await this.open(null);
      await this.refreshList();
    } catch (e) {
      toast.error(e);
    }
  }

  replaceProposal(p: Proposal) {
    this.proposals = this.proposals.map((x) => (x.id === p.id ? p : x));
  }

  /** Proposals created by the tool calls in an assistant message. */
  proposalsFor(blocks: Block[]): Proposal[] {
    const ids = new Set(blocks.filter((b) => b.type === "tool_use").map((b) => (b as { id: string }).id));
    return this.proposals.filter((p) => p.preview.tool_use_id && ids.has(p.preview.tool_use_id));
  }
}

export const blocksOf = (m: StoredMessage): Block[] =>
  Array.isArray(m.content) ? m.content : [{ type: "text", text: String(m.content) }];

/**
 * Visible text of a message. Tool results and thinking are not chat. For user messages only the
 * last text block is what the person typed: earlier ones are context and proposal outcomes that
 * the app injected for the model.
 */
export const textOf = (m: StoredMessage): string => {
  const texts = blocksOf(m)
    .filter((b): b is Extract<Block, { type: "text" }> => b.type === "text")
    .map((b) => b.text);
  return m.role === "user" ? (texts[texts.length - 1] ?? "") : texts.join("\n\n");
};
