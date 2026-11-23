import { call } from "./invoke";
import type { ActivityEntry, Setting } from "./types";

export const getSetting = (key: string) => call<Setting | null>("get_setting", { key });
export const listSettings = () => call<Setting[]>("list_settings");
export const setSetting = (key: string, value: unknown) =>
  call<Setting>("set_setting", { key, value: JSON.stringify(value) });
export const recentActivity = (limit = 50) => call<ActivityEntry[]>("recent_activity", { limit });
