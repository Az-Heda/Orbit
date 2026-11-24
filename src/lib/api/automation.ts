import { call } from "./invoke";

export type Trigger =
  | { kind: "task_created" }
  | { kind: "task_moved"; to_column_kind?: string | null; to_column_id?: string | null }
  | { kind: "task_completed" }
  | { kind: "timer_ended" }
  | { kind: "inbox_added" }
  | { kind: "schedule"; time: string; days: number[]; timezone: string };

export type Action =
  | { kind: "create_task"; column_id: string; title: string; description?: string | null }
  | { kind: "create_note"; title: string; body: string }
  | { kind: "move_task"; column_id: string }
  | { kind: "start_timer"; timer_kind: "pomodoro" | "countdown"; minutes: number; label?: string | null }
  | { kind: "notify"; title: string; body?: string | null }
  | { kind: "send_notification"; channel_id: string };

/** A rule runs at most this many steps. */
export const MAX_STEPS = 10;

export interface Rule {
  id: string;
  name: string;
  enabled: boolean;
  trigger: Trigger;
  /** The chain: steps run in order, a failing step stops the chain. */
  actions: Action[];
}
export interface RuleInput {
  name: string;
  enabled?: boolean;
  trigger: Trigger;
  actions: Action[];
}

export const listRules = () => call<Rule[]>("list_rules");
export const createRule = (input: RuleInput) => call<Rule>("create_rule", { input });
export const updateRule = (id: string, input: RuleInput) => call<Rule>("update_rule", { id, input });
export const setRuleEnabled = (id: string, enabled: boolean) => call<Rule>("set_rule_enabled", { id, enabled });
export const deleteRule = (id: string) => call<void>("delete_rule", { id });

export function describeTrigger(t: Trigger): string {
  switch (t.kind) {
    case "task_created": return "a task is created";
    case "task_moved": return t.to_column_kind ? `a task moves to a ${t.to_column_kind} column` : "a task is moved";
    case "task_completed": return "a task is completed";
    case "timer_ended": return "a timer ends";
    case "inbox_added": return "an inbox item is added";
    case "schedule": {
      const names = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
      const days = t.days.length ? t.days.map((d) => names[d - 1]).join(", ") : "every day";
      return `${days} at ${t.time} (${t.timezone})`;
    }
  }
}
export function describeAction(a: Action, channelName?: (id: string) => string | undefined): string {
  switch (a.kind) {
    case "create_task": return `create task "${a.title}"`;
    case "create_note": return `create note "${a.title}"`;
    case "move_task": return "move that task to a column";
    case "start_timer": return `start a ${a.minutes} min ${a.timer_kind} timer`;
    case "notify": return `notify "${a.title}"`;
    case "send_notification": return `send a message to ${channelName?.(a.channel_id) ? `"${channelName(a.channel_id)}"` : "a deleted channel"}`;
  }
}

export function describeChain(actions: Action[], channelName?: (id: string) => string | undefined): string {
  return actions.map((a) => describeAction(a, channelName)).join(", then ");
}
