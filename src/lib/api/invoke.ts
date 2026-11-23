import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import type { AppError } from "./types";

/** Normalised error thrown by every typed command wrapper. */
export class CommandError extends Error {
  kind: AppError["kind"] | "unknown";
  constructor(kind: CommandError["kind"], message: string) {
    super(message);
    this.kind = kind;
  }
}

export async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await tauriInvoke<T>(cmd, args);
  } catch (e) {
    const err = e as Partial<AppError>;
    throw new CommandError(err?.kind ?? "unknown", err?.message ?? String(e));
  }
}
