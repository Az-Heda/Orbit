import { call } from "./invoke";

export interface Conversation {
  id: string;
  title: string;
  created_at: string;
  updated_at: string;
}

export type Block =
  | { type: "text"; text: string }
  | { type: "thinking" }
  | { type: "tool_use"; id: string; name: string; input: unknown }
  | { type: "tool_result"; tool_use_id: string; content: string; is_error?: boolean };

export interface StoredMessage {
  id: string;
  seq: number;
  role: "user" | "assistant";
  content: Block[] | string;
  created_at: string;
}

export interface Change {
  label: string;
  before: string | null;
  after: string | null;
}

export interface Preview {
  summary: string;
  destructive: boolean;
  changes: Change[];
  timezone?: string;
  tool_use_id?: string;
}

export interface Proposal {
  id: string;
  conversation_id: string;
  tool_name: string;
  input: unknown;
  preview: Preview;
  status: "pending" | "applied" | "rejected" | "failed" | "undone";
  result: { error?: string; title?: string } | null;
  created_at: string;
  resolved_at: string | null;
}

export interface ConversationDetail {
  conversation: Conversation;
  messages: StoredMessage[];
  proposals: Proposal[];
}

export interface ChatContext {
  view?: string | null;
  timezone?: string | null;
  project_id?: string | null;
  board_id?: string | null;
  task_id?: string | null;
  event_id?: string | null;
  note_id?: string | null;
}

export type ChatEvent =
  | { kind: "started"; conversation_id: string }
  | { kind: "text_delta"; text: string }
  | { kind: "tool_started"; name: string }
  | { kind: "proposal"; proposal: Proposal }
  | { kind: "done"; conversation_id: string }
  | { kind: "error"; message: string };

export interface KeyStatus {
  configured: boolean;
  /** "credential_manager" | "environment" | null */
  source: string | null;
  /** false where no secure store exists (non-Windows): the key can only come from ANTHROPIC_API_KEY. */
  can_store: boolean;
}

export interface AiConfig {
  model: string;
  effort: string;
}

export interface Sharing {
  tasks: boolean;
  calendar: boolean;
  notes: boolean;
  inbox: boolean;
  memories: boolean;
  focus: boolean;
}

export interface Memory {
  id: string;
  text: string;
  created_at: string;
  updated_at: string;
}

export const sendMessage = (conversationId: string | null, text: string, context: ChatContext) =>
  call<string>("ai_send_message", { conversationId, text, context });
export const cancelMessage = (conversationId: string) => call<void>("ai_cancel", { conversationId });
export const listConversations = () => call<Conversation[]>("ai_list_conversations");
export const getConversation = (id: string) => call<ConversationDetail>("ai_get_conversation", { id });
export const deleteConversation = (id: string) => call<void>("ai_delete_conversation", { id });
export const applyProposal = (id: string) => call<Proposal>("ai_apply_proposal", { id });
export const rejectProposal = (id: string) => call<Proposal>("ai_reject_proposal", { id });
export const undoProposal = (id: string) => call<[Proposal, { reverted: number }]>("ai_undo_proposal", { id });

export const keyStatus = () => call<KeyStatus>("ai_key_status");
export const setKey = (key: string) => call<KeyStatus>("ai_set_key", { key });
export const clearKey = () => call<KeyStatus>("ai_clear_key");
export const getConfig = () => call<AiConfig>("ai_get_config");
export const setConfig = (config: AiConfig) => call<AiConfig>("ai_set_config", { config });
export const getSharing = () => call<Sharing>("ai_get_sharing");
export const setSharing = (sharing: Sharing) => call<Sharing>("ai_set_sharing", { sharing });
export const listMemories = () => call<Memory[]>("ai_list_memories");
export const addMemory = (text: string) => call<Memory>("ai_add_memory", { text });
export const updateMemory = (id: string, text: string) => call<Memory>("ai_update_memory", { id, text });
export const deleteMemory = (id: string) => call<void>("ai_delete_memory", { id });
