import { defineStore } from "pinia";

export type AlertKind = "success" | "error";

export interface AlertMessage {
  id: number;
  kind: AlertKind;
  title: string;
  message?: string;
}

const DEFAULT_DISMISS_MS = 6000;

export const useAlertsStore = defineStore("alerts", {
  state: () => ({
    alerts: [] as AlertMessage[],
    counter: 0,
  }),
  actions: {
    push(kind: AlertKind, title: string, message?: string, dismissAfterMs = DEFAULT_DISMISS_MS) {
      const id = ++this.counter;
      this.alerts.push({ id, kind, title, message });

      if (dismissAfterMs > 0) {
        window.setTimeout(() => {
          this.dismiss(id);
        }, dismissAfterMs);
      }

      return id;
    },
    success(title: string, message?: string, dismissAfterMs = DEFAULT_DISMISS_MS) {
      return this.push("success", title, message, dismissAfterMs);
    },
    error(title: string, message?: string, dismissAfterMs = DEFAULT_DISMISS_MS) {
      return this.push("error", title, message, dismissAfterMs);
    },
    dismiss(id: number) {
      this.alerts = this.alerts.filter((alert) => alert.id !== id);
    },
    clear() {
      this.alerts = [];
    },
  },
});
