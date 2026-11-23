class ToastState {
  message = $state<string | null>(null);
  private timer: ReturnType<typeof setTimeout> | undefined;

  error(e: unknown) {
    this.message = e instanceof Error ? e.message : String(e);
    clearTimeout(this.timer);
    this.timer = setTimeout(() => (this.message = null), 6000);
  }
  dismiss() {
    this.message = null;
  }
}
export const toast = new ToastState();
