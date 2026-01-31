/**
 * Toast Notification System
 *
 * Provides a simple way to show toast notifications from anywhere in the app.
 */

export type ToastType = "success" | "error" | "warning" | "info";

export interface Toast {
  id: string;
  type: ToastType;
  title: string;
  message?: string;
  duration: number;
  action?: {
    label: string;
    onClick: () => void;
    /** Keyboard shortcut to trigger the action (e.g., "r" for pressing R key) */
    shortcut?: string;
  };
}

export interface ToastOptions {
  type?: ToastType;
  title: string;
  message?: string;
  duration?: number;
  action?: {
    label: string;
    onClick: () => void;
    /** Keyboard shortcut to trigger the action (e.g., "r" for pressing R key) */
    shortcut?: string;
  };
}

// Default durations by type (in ms)
const DEFAULT_DURATIONS: Record<ToastType, number> = {
  success: 4000,
  error: 6000,
  warning: 5000,
  info: 4000,
};

// Store for active toasts
let toasts: Toast[] = $state([]);

// Subscribers for toast changes
type ToastSubscriber = (toasts: Toast[]) => void;
const subscribers: Set<ToastSubscriber> = new Set();

function notifySubscribers() {
  subscribers.forEach((callback) => callback(toasts));
}

/**
 * Generate unique ID for toast
 */
function generateId(): string {
  return `toast-${Date.now()}-${Math.random().toString(36).substring(2, 9)}`;
}

/**
 * Show a toast notification
 */
export function showToast(options: ToastOptions): string {
  const id = generateId();
  const type = options.type || "info";
  const duration = options.duration ?? DEFAULT_DURATIONS[type];

  const toast: Toast = {
    id,
    type,
    title: options.title,
    message: options.message,
    duration,
    action: options.action,
  };

  toasts = [...toasts, toast];
  notifySubscribers();

  // Auto-dismiss after duration (unless duration is 0)
  if (duration > 0) {
    setTimeout(() => {
      dismissToast(id);
    }, duration);
  }

  return id;
}

/**
 * Dismiss a toast by ID
 */
export function dismissToast(id: string): void {
  toasts = toasts.filter((t) => t.id !== id);
  notifySubscribers();
}

/**
 * Dismiss all toasts
 */
export function dismissAllToasts(): void {
  toasts = [];
  notifySubscribers();
}

/**
 * Get current toasts
 */
export function getToasts(): Toast[] {
  return toasts;
}

/**
 * Subscribe to toast changes
 */
export function subscribeToToasts(callback: ToastSubscriber): () => void {
  subscribers.add(callback);
  // Immediately call with current toasts
  callback(toasts);
  return () => subscribers.delete(callback);
}

// Convenience methods
export const toast = {
  success: (title: string, message?: string) =>
    showToast({ type: "success", title, message }),
  error: (title: string, message?: string) =>
    showToast({ type: "error", title, message }),
  warning: (title: string, message?: string) =>
    showToast({ type: "warning", title, message }),
  info: (title: string, message?: string) =>
    showToast({ type: "info", title, message }),
  show: showToast,
  dismiss: dismissToast,
  dismissAll: dismissAllToasts,
};
