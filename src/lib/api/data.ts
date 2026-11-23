import { call } from "./invoke";

export interface BackupInfo {
  name: string;
  kind: "auto" | "manual" | "pre-import" | "pre-update";
  size_bytes: number;
  created_at: string;
}
export interface BackupStatus {
  enabled: boolean;
  keep: number;
  import_pending: boolean;
  folder: string;
}

export const backupStatus = () => call<BackupStatus>("backup_status");
export const backupList = () => call<BackupInfo[]>("backup_list");
export const backupNow = () => call<BackupInfo>("backup_now");
export const backupExport = (path: string) => call<void>("backup_export", { path });
export const backupImport = (path: string) => call<void>("backup_import", { path });
export const backupRestore = (name: string) => call<void>("backup_restore", { name });
export const backupCancelImport = () => call<void>("backup_cancel_import");
export const restartApp = () => call<void>("restart_app");

export interface ReleaseInfo {
  tag: string;
  version: string;
  notes: string;
  page_url: string;
}
export interface UpdateInfo {
  current: string;
  enabled: boolean;
  available: boolean;
  skipped: boolean;
  release: ReleaseInfo | null;
  /** Null when Orbit can install the update itself. */
  install_blocker: string | null;
}
export const updateCheck = (force: boolean) => call<UpdateInfo>("update_check", { force });
export const updateSkip = (version: string) => call<void>("update_skip", { version });
export const updateInstall = () => call<void>("update_install");
