import { call } from "./invoke";

export interface TimerView {
  id: string;
  kind: "pomodoro" | "countdown" | "stopwatch";
  label: string | null;
  task_id: string | null;
  duration_ms: number | null;
  phase: "focus" | "break";
  state: "running" | "paused" | "finished" | "cancelled";
  started_at: string;
  ended_at: string | null;
  elapsed_ms: number;
  remaining_ms: number | null;
  now: string;
}
export interface FocusSession {
  id: string;
  timer_id: string | null;
  task_id: string | null;
  started_at: string;
  ended_at: string;
  duration_ms: number;
}
export interface DayStat { date: string; focus_ms: number; sessions: number; completed: number }
export interface ProjectStat { project_id: string | null; name: string; focus_ms: number }
export interface Stats {
  timezone: string;
  from: string;
  to: string;
  days: DayStat[];
  by_project: ProjectStat[];
  total_focus_ms: number;
  total_completed: number;
  current_streak: number;
  longest_streak: number;
}
export interface StartTimerInput {
  kind: TimerView["kind"];
  label?: string;
  task_id?: string;
  duration_ms?: number;
  phase?: "focus" | "break";
}

export const startTimer = (input: StartTimerInput) => call<TimerView>("start_timer", { input });
export const pauseTimer = (id: string) => call<TimerView>("pause_timer", { id });
export const resumeTimer = (id: string) => call<TimerView>("resume_timer", { id });
export const stopTimer = (id: string) => call<TimerView>("stop_timer", { id });
export const activeTimers = () => call<TimerView[]>("active_timers");
export const timerHistory = (limit = 30) => call<TimerView[]>("timer_history", { limit });
export const listFocusSessions = (from: string, to: string, taskId?: string) =>
  call<FocusSession[]>("list_focus_sessions", { from, to, taskId });
export const getStats = (timezone: string, days: number) => call<Stats>("get_stats", { timezone, days });
