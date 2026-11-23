import { CommandError } from "$lib/api/invoke";

interface Pending {
  message: string;
  confirmLabel: string;
  resolve: (ok: boolean) => void;
}

class ConfirmState {
  pending = $state<Pending | null>(null);

  ask(message: string, confirmLabel = "Continue anyway"): Promise<boolean> {
    return new Promise((resolve) => {
      this.pending = { message, confirmLabel, resolve };
    });
  }

  answer(ok: boolean) {
    this.pending?.resolve(ok);
    this.pending = null;
  }
}

export const confirmState = new ConfirmState();

/**
 * Run an action; if the backend answers `requires_confirmation`, ask the user and retry with
 * `force = true`. Returns undefined when the user declines.
 */
export async function withConfirm<T>(action: (force: boolean) => Promise<T>): Promise<T | undefined> {
  try {
    return await action(false);
  } catch (e) {
    if (e instanceof CommandError && e.kind === "requires_confirmation") {
      const ok = await confirmState.ask(e.message.replace(/^confirmation required: /, ""));
      return ok ? action(true) : undefined;
    }
    throw e;
  }
}
