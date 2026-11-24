import { call } from "./invoke";

export type ChannelKind = "windows" | "telegram" | "discord";

/** What the app may know about a channel. The token / webhook is never sent back. */
export interface Channel {
  id: string;
  name: string;
  kind: ChannelKind;
  /** Telegram only. */
  chat_id: string | null;
  template: string;
  /** A token or webhook URL is stored in the credential store. */
  has_secret: boolean;
}

export interface ChannelInput {
  name: string;
  kind: ChannelKind;
  template: string;
  chat_id?: string | null;
  /** Bot token (Telegram) or webhook URL (Discord). Omit on update to keep the stored one. */
  secret?: string | null;
}

export const KIND_LABELS: Record<ChannelKind, string> = {
  windows: "Windows notification",
  telegram: "Telegram",
  discord: "Discord",
};

/** Placeholders a template can use (same list the backend understands). */
export const PLACEHOLDERS = [
  ["{{title}}", "the item that triggered the rule"],
  ["{{task.title}}", "task title (task triggers)"],
  ["{{timer.label}}", "timer label (timer trigger)"],
  ["{{inbox.text}}", "inbox text (inbox trigger)"],
  ["{{rule.name}}", "name of the rule"],
  ["{{trigger}}", "trigger kind, e.g. task_completed"],
  ["{{date}}", "today (UTC date)"],
] as const;

export const DEFAULT_TEMPLATE = "{{rule.name}}: {{title}}";

export const listChannels = () => call<Channel[]>("list_channels");
export const createChannel = (input: ChannelInput) => call<Channel>("create_channel", { input });
export const updateChannel = (id: string, input: ChannelInput) => call<Channel>("update_channel", { id, input });
export const deleteChannel = (id: string) => call<void>("delete_channel", { id });
export const sendTestNotification = (id: string) => call<void>("send_test_notification", { id });
