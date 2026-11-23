import { invoke } from "@tauri-apps/api/core";
import type { ActivityEntry, Setting } from "./types";

export const getSetting = (key: string) => invoke<Setting | null>("get_setting", { key });
export const listSettings = () => invoke<Setting[]>("list_settings");
export const setSetting = (key: string, value: unknown) =>
  invoke<Setting>("set_setting", { key, value: JSON.stringify(value) });
export const recentActivity = (limit = 50) => invoke<ActivityEntry[]>("recent_activity", { limit });
