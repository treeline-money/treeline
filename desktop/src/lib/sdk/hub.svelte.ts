/**
 * Hub watch state - in-process background watch loop.
 *
 * The Tauri backend runs the watch loop on a worker thread (see
 * `start_hub_watch` in src-tauri/src/lib.rs). It emits a `hub-watch-event`
 * for each step (push started, pushed, pulled, conflict, etc.). This module
 * subscribes to that stream and exposes a Svelte 5 rune-backed store the UI
 * can read.
 */

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { registry } from "./registry";
import { toast } from "./toast.svelte";

export type WatchEvent =
  | { kind: "started"; hub_url: string }
  | { kind: "ready" }
  | { kind: "local_change_detected" }
  | { kind: "pushing" }
  | { kind: "pushed"; bytes: number }
  | { kind: "auto_merged"; bytes: number }
  | { kind: "pulling" }
  | { kind: "pulled"; bytes: number }
  | { kind: "conflict"; hub_hash: string; conflicts: number }
  | { kind: "no_base_snapshot"; hub_hash: string }
  | { kind: "error"; message: string }
  | { kind: "stopped" };

export type WatchStatus =
  | "idle"
  | "watching"
  | "pushing"
  | "pulling"
  | "up_to_date"
  | "conflict"
  | "error"
  | "stopped";

class HubWatchStore {
  private _running = $state(false);
  private _status = $state<WatchStatus>("idle");
  private _hubUrl = $state<string | null>(null);
  private _lastEvent = $state<WatchEvent | null>(null);
  private _lastUpdatedAt = $state<number | null>(null);
  private _errorMessage = $state<string | null>(null);
  private _conflictCount = $state(0);
  private _ready = $state(false);
  private _unlisten: UnlistenFn | null = null;
  private readyWaiters: (() => void)[] = [];
  /** The watcher re-reports an unresolved conflict every poll cycle —
   *  raise one persistent toast on first detection (dismissed when the
   *  conflict clears), re-arm once it does. */
  private conflictToastId: string | null = null;

  get running() {
    return this._running;
  }
  get status() {
    return this._status;
  }
  get hubUrl() {
    return this._hubUrl;
  }
  get lastEvent() {
    return this._lastEvent;
  }
  get lastUpdatedAt() {
    return this._lastUpdatedAt;
  }
  get errorMessage() {
    return this._errorMessage;
  }
  get conflictCount() {
    return this._conflictCount;
  }
  get ready() {
    return this._ready;
  }

  /**
   * Resolve once the watcher's initial reconcile is done (hub pulled if it
   * had moved and local was clean), so startup writers like the automatic
   * bank sync begin from the hub's latest state. Resolves immediately when
   * no watcher is running, and after `timeoutMs` at the latest — an
   * unreachable hub must never block local work.
   */
  async waitForReady(timeoutMs = 5000): Promise<void> {
    if (!this._running || this._ready) return;
    await new Promise<void>((resolve) => {
      const timer = setTimeout(resolve, timeoutMs);
      this.readyWaiters.push(() => {
        clearTimeout(timer);
        resolve();
      });
    });
  }

  /**
   * Subscribe to backend `hub-watch-event` notifications. Idempotent.
   */
  async subscribe(): Promise<void> {
    if (this._unlisten) return;
    this._unlisten = await listen<WatchEvent>("hub-watch-event", (e) => {
      this.applyEvent(e.payload);
    });
  }

  async unsubscribe(): Promise<void> {
    if (this._unlisten) {
      this._unlisten();
      this._unlisten = null;
    }
  }

  /**
   * Start the in-process watcher. Resolves to true if started, false if
   * one was already running. Returns the error string from the backend
   * (e.g. "Not linked to a hub.") if start failed, after silently swallowing.
   */
  async start(): Promise<boolean> {
    try {
      await this.subscribe();
      const started = await invoke<boolean>("start_hub_watch");
      this._running = await invoke<boolean>("hub_watch_status");
      return started;
    } catch (e) {
      // Most common case: not linked. Don't surface as error to user.
      console.debug("[hub-watch] start failed:", e);
      return false;
    }
  }

  /**
   * Stop the in-process watcher. Resolves to true if a watcher was running.
   */
  async stop(): Promise<boolean> {
    try {
      const stopped = await invoke<boolean>("stop_hub_watch");
      this._running = await invoke<boolean>("hub_watch_status");
      return stopped;
    } catch (e) {
      console.error("[hub-watch] stop failed:", e);
      return false;
    }
  }

  /**
   * Refresh `running` from the backend (in case a CLI watcher is also active
   * or the backend lost track).
   */
  async refresh(): Promise<void> {
    try {
      this._running = await invoke<boolean>("hub_watch_status");
    } catch (e) {
      console.debug("[hub-watch] refresh failed:", e);
    }
  }

  private notifyConflict(): void {
    if (this.conflictToastId) return;
    // Persistent (duration 0) — a conflict shouldn't vanish after 5s.
    // Dismissed when the conflict clears, or by resolving via the action.
    this.conflictToastId = toast.show({
      type: "warning",
      title: "Sync conflict",
      message:
        "This device and the hub changed the same data in different ways. " +
        "Nothing syncs until you pick which side wins.",
      duration: 0,
      action: {
        label: "Review & resolve",
        onClick: () => registry.emit("hub:conflict:open"),
      },
    });
  }

  /** Dismiss the persistent conflict toast (no-op if none). Called when
   *  the conflict clears via sync events or an explicit resolution. */
  clearConflictNotification(): void {
    if (this.conflictToastId) {
      toast.dismiss(this.conflictToastId);
      this.conflictToastId = null;
    }
  }

  private applyEvent(event: WatchEvent): void {
    this._lastEvent = event;
    switch (event.kind) {
      case "started":
        this._hubUrl = event.hub_url;
        this._status = "watching";
        this._errorMessage = null;
        this._conflictCount = 0;
        this._ready = false;
        break;
      case "ready":
        this._ready = true;
        this.readyWaiters.splice(0).forEach((resolve) => resolve());
        break;
      case "local_change_detected":
      case "pushing":
        this._status = "pushing";
        break;
      case "pulling":
        this._status = "pulling";
        break;
      case "pushed":
      case "auto_merged":
      case "pulled":
        this._status = "up_to_date";
        this._lastUpdatedAt = Date.now();
        this._errorMessage = null;
        this._conflictCount = 0;
        this.clearConflictNotification();
        // Pulls and merges rewrote the local DB — views showing pre-sync
        // data need to reload.
        if (event.kind !== "pushed") {
          registry.emit("data:refresh");
        }
        break;
      case "conflict":
        this._status = "conflict";
        this._conflictCount = event.conflicts;
        this.notifyConflict();
        break;
      case "no_base_snapshot":
        this._status = "conflict";
        this._conflictCount = 1;
        this.notifyConflict();
        break;
      case "error":
        this._status = "error";
        this._errorMessage = event.message;
        break;
      case "stopped":
        this._status = "stopped";
        this._running = false;
        this._ready = false;
        this.readyWaiters.splice(0).forEach((resolve) => resolve());
        break;
    }
  }
}

export const hubWatch = new HubWatchStore();

// ============================================================================
// Hub link (device-code OAuth)
// ============================================================================

export interface HubLinkInfo {
  user_code: string;
  verification_uri: string;
  verification_uri_complete: string;
  interval: number;
  url: string;
  device_name: string;
}

export type HubLinkPollResult =
  | { status: "pending" }
  | { status: "slow_down" }
  | { status: "linked"; hub_url: string; device_name: string }
  | { status: "expired" }
  | { status: "denied" };

export interface HubLinkStatus {
  url: string;
  device_name: string;
  last_push: string | null;
  last_pull: string | null;
  /** The URL the user originally pointed at when linking. Can differ from
   *  `url` if the hub redirects the device to a different address after the
   *  link completes. Null for legacy hub.json files written before this
   *  field existed. */
  link_origin: string | null;
}

/** Start a device-code link against the given hub. Returns the user-code +
 *  verification URL the UI should display. Frontend then polls
 *  `pollHubLink` every `interval` seconds until linked / expired. */
export async function startHubLink(url: string, deviceName: string): Promise<HubLinkInfo> {
  return invoke<HubLinkInfo>("start_hub_link", { url, deviceName });
}

export async function pollHubLink(): Promise<HubLinkPollResult> {
  return invoke<HubLinkPollResult>("poll_hub_link");
}

export async function cancelHubLink(): Promise<void> {
  return invoke("cancel_hub_link");
}

export async function unlinkHub(): Promise<void> {
  return invoke("unlink_hub");
}

export async function getHubLinkStatus(): Promise<HubLinkStatus | null> {
  return invoke<HubLinkStatus | null>("get_hub_link_status");
}

/** Outcome of `pushToHubNow`. */
export type HubPushNowResult =
  | { status: "pushed"; bytes: number }
  | { status: "auto_merged"; bytes: number }
  | { status: "conflict" }
  | { status: "no_base_snapshot" }
  | { status: "no_changes" };

/** One-shot push to the linked hub. Used right after linking so the local
 *  DB lands on the hub immediately — the device is the source of truth. */
export async function pushToHubNow(): Promise<HubPushNowResult> {
  return invoke<HubPushNowResult>("push_to_hub_now");
}

// ============================================================================
// Conflict inspection & resolution
// ============================================================================

export interface ColumnConflict {
  column: string;
  base: unknown;
  local: unknown;
  hub: unknown;
}

export type RowConflict =
  | { kind: "modified"; key: Record<string, unknown>; columns: ColumnConflict[] }
  | {
      kind: "both_added";
      local_row: Record<string, unknown>;
      hub_row: Record<string, unknown>;
    }
  | {
      kind: "delete_vs_modify";
      deleted_by: "local" | "hub" | null;
      deleted_row: Record<string, unknown>;
      modified_row: Record<string, unknown>;
    };

export interface TableConflicts {
  table: string;
  conflicts: RowConflict[];
}

export interface ConflictReport {
  hub_hash: string;
  /** Rows changed only on one side — these merge cleanly either way. */
  local_only_changes: number;
  hub_only_changes: number;
  total_conflicts: number;
  tables: TableConflicts[];
}

/** Row/column-level conflict detail. Read-only; takes a few seconds (it
 *  downloads the hub bundle and diffs it). */
export async function getHubConflicts(): Promise<ConflictReport> {
  return invoke<ConflictReport>("get_hub_conflicts");
}

export type HubResolveResult =
  | { status: "resolved"; bytes: number }
  | { status: "no_changes" }
  | { status: "no_base_snapshot" };

/** Resolve conflicts by choosing which side wins conflicting values. The
 *  losing side's non-conflicting changes still merge in. Stops the watcher
 *  for the duration; the restart happens in the background so callers (the
 *  modal) aren't kept waiting after the push has already landed — the old
 *  watcher thread can hold its lock for a few seconds after stop, hence
 *  the deliberately patient retry loop. */
export async function resolveHubConflicts(keep: "local" | "hub"): Promise<HubResolveResult> {
  const wasRunning = await hubWatch.stop();
  try {
    const result = await invoke<HubResolveResult>("resolve_hub_conflicts", { keep });
    hubWatch.clearConflictNotification();
    return result;
  } finally {
    if (wasRunning) {
      void (async () => {
        for (let i = 0; i < 20; i++) {
          if (await hubWatch.start()) break;
          await new Promise((resolve) => setTimeout(resolve, 500));
        }
      })();
    }
  }
}
