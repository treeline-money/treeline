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

export type WatchEvent =
  | { kind: "started"; hub_url: string }
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
  private _unlisten: UnlistenFn | null = null;

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

  private applyEvent(event: WatchEvent): void {
    this._lastEvent = event;
    switch (event.kind) {
      case "started":
        this._hubUrl = event.hub_url;
        this._status = "watching";
        this._errorMessage = null;
        this._conflictCount = 0;
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
        // Pulls and merges rewrote the local DB — views showing pre-sync
        // data need to reload.
        if (event.kind !== "pushed") {
          registry.emit("data:refresh");
        }
        break;
      case "conflict":
        this._status = "conflict";
        this._conflictCount = event.conflicts;
        break;
      case "no_base_snapshot":
        this._status = "conflict";
        this._conflictCount = 1;
        break;
      case "error":
        this._status = "error";
        this._errorMessage = event.message;
        break;
      case "stopped":
        this._status = "stopped";
        this._running = false;
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
