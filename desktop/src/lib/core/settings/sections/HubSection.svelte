<script lang="ts">
  import { onMount } from "svelte";
  import { Icon } from "../../../shared";
  import {
    getHubLinkStatus,
    unlinkHub,
    hubWatch,
    toast,
    type HubLinkStatus,
    type WatchStatus,
  } from "../../../sdk";
  import HubLinkModal from "../HubLinkModal.svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import "../settings-shared.css";

  const CLOUD_DASHBOARD_URL = "https://pro.treeline.money/welcome";

  let status = $state<HubLinkStatus | null>(null);
  let loading = $state(true);
  let showLinkModal = $state(false);
  let unlinking = $state(false);

  // Tick once a minute so "Last pushed 5m ago" doesn't go stale.
  let now = $state(Date.now());
  $effect(() => {
    const id = setInterval(() => (now = Date.now()), 60_000);
    return () => clearInterval(id);
  });

  async function refreshStatus() {
    try {
      status = await getHubLinkStatus();
    } catch (e) {
      console.error("Failed to load hub status:", e);
    } finally {
      loading = false;
    }
  }

  onMount(refreshStatus);

  // Re-read hub.json whenever the watcher reports a successful push/pull —
  // that's when last_push / last_pull get updated on disk.
  let lastWatcherUpdate = $derived(hubWatch.lastUpdatedAt);
  $effect(() => {
    if (lastWatcherUpdate !== null && status) {
      refreshStatus();
    }
  });

  async function handleUnlink() {
    if (!confirm("Unlink this device from the hub? Local data stays; nothing is deleted.")) {
      return;
    }
    unlinking = true;
    try {
      await hubWatch.stop();
      await unlinkHub();
      await refreshStatus();
      toast.success("Unlinked");
    } catch (e) {
      toast.error(`Unlink failed: ${e}`);
    } finally {
      unlinking = false;
    }
  }

  function timeAgo(d: string | null, _tick: number): string {
    if (!d) return "never";
    const ms = _tick - new Date(d).getTime();
    const seconds = Math.max(0, Math.floor(ms / 1000));
    if (seconds < 5) return "just now";
    if (seconds < 60) return `${seconds}s ago`;
    const minutes = Math.floor(seconds / 60);
    if (minutes < 60) return `${minutes}m ago`;
    const hours = Math.floor(minutes / 60);
    if (hours < 24) return `${hours}h ago`;
    const days = Math.floor(hours / 24);
    return `${days}d ago`;
  }

  function isCloud(s: HubLinkStatus): boolean {
    // Prefer `link_origin` because `url` gets overwritten with the hub's
    // direct (Fly) URL after Pro completes the link. Fall back to `url`
    // for legacy hub.json files written before `link_origin` existed.
    const candidate = s.link_origin ?? s.url;
    try {
      return new URL(candidate).hostname === "pro.treeline.money";
    } catch {
      return false;
    }
  }

  function hubDisplayName(s: HubLinkStatus): string {
    return isCloud(s) ? "Treeline Cloud" : "Self-hosted hub";
  }

  type StatusKind = "ok" | "active" | "warn" | "error" | "neutral";

  function watcherDisplay(
    running: boolean,
    state: WatchStatus,
  ): { kind: StatusKind; label: string } {
    if (!running) return { kind: "neutral", label: "Watcher not running" };
    switch (state) {
      case "watching":
        return { kind: "ok", label: "Watching" };
      case "pushing":
        return { kind: "active", label: "Pushing…" };
      case "pulling":
        return { kind: "active", label: "Pulling…" };
      case "up_to_date":
        return { kind: "ok", label: "Up to date" };
      case "conflict":
        return { kind: "warn", label: "Conflict — resolve via CLI" };
      case "error":
        return { kind: "error", label: hubWatch.errorMessage || "Hub error" };
      case "stopped":
        return { kind: "neutral", label: "Watcher stopped" };
      default:
        return { kind: "neutral", label: state };
    }
  }

  let watcherInfo = $derived(watcherDisplay(hubWatch.running, hubWatch.status));
</script>

<section class="section">
  <h3 class="section-title">Hub</h3>
  <p class="group-desc">
    Link this device to a Treeline hub so your database flows between your devices automatically.
  </p>

  {#if loading}
    <p class="group-desc">Checking…</p>
  {:else if status}
    <div class="hub-card">
      <div class="hub-card-header">
        <div class="hub-status">
          <span class="status-dot {watcherInfo.kind}"></span>
          <div class="hub-status-text">
            <h4 class="hub-name">Connected to {hubDisplayName(status)}</h4>
            <span class="hub-substate">{watcherInfo.label}</span>
          </div>
        </div>
      </div>

      <div class="hub-meta">
        <div class="meta-row">
          <span class="meta-label">Device</span>
          <span class="meta-value">{status.device_name}</span>
        </div>
        {#if !isCloud(status)}
          <div class="meta-row">
            <span class="meta-label">Hub</span>
            <code class="meta-value">{status.url}</code>
          </div>
        {/if}
      </div>

      {#if isCloud(status)}
        <button class="manage-link" onclick={() => openUrl(CLOUD_DASHBOARD_URL)}>
          Manage in Treeline Cloud
          <Icon name="external-link" size={12} />
        </button>
      {/if}

      <div class="hub-divider"></div>

      <div class="hub-activity">
        <div class="activity-row">
          <Icon name="arrow-up-circle" size={14} />
          <span class="activity-label">Last push</span>
          <span class="activity-value">{timeAgo(status.last_push, now)}</span>
        </div>
        <div class="activity-row">
          <Icon name="download" size={14} />
          <span class="activity-label">Last pull</span>
          <span class="activity-value">{timeAgo(status.last_pull, now)}</span>
        </div>
      </div>
    </div>

    <div class="danger-zone">
      <button class="btn secondary small" onclick={handleUnlink} disabled={unlinking}>
        {unlinking ? "Unlinking…" : "Unlink device"}
      </button>
    </div>
  {:else}
    <div class="empty">
      <span>This device isn't linked to a hub yet.</span>
      <button class="btn primary" onclick={() => (showLinkModal = true)}>
        <Icon name="link" size={14} />
        Link a hub
      </button>
    </div>
  {/if}
</section>

<HubLinkModal
  isOpen={showLinkModal}
  onLinked={refreshStatus}
  onClose={() => (showLinkModal = false)}
/>

<style>
  /* Linked-state card */
  .hub-card {
    background: var(--bg-secondary);
    border: 1px solid var(--border-primary);
    border-radius: 8px;
    padding: var(--spacing-md);
    margin-bottom: var(--spacing-md);
  }

  .hub-card-header {
    margin-bottom: var(--spacing-md);
  }

  .hub-status {
    display: flex;
    align-items: center;
    gap: var(--spacing-sm);
  }

  .status-dot {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    flex-shrink: 0;
  }
  .status-dot.ok {
    background: var(--accent-success, #22c55e);
    box-shadow: 0 0 0 3px rgba(34, 197, 94, 0.18);
  }
  .status-dot.active {
    background: var(--accent-primary);
    box-shadow: 0 0 0 3px rgba(59, 130, 246, 0.2);
    animation: pulse 1.4s ease-in-out infinite;
  }
  .status-dot.warn {
    background: var(--accent-warning, #f59e0b);
    box-shadow: 0 0 0 3px rgba(245, 158, 11, 0.2);
  }
  .status-dot.error {
    background: var(--accent-danger, #dc2626);
    box-shadow: 0 0 0 3px rgba(220, 38, 38, 0.2);
  }
  .status-dot.neutral {
    background: var(--text-muted);
  }
  @keyframes pulse {
    0%, 100% {
      opacity: 1;
    }
    50% {
      opacity: 0.55;
    }
  }

  .hub-status-text {
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .hub-name {
    margin: 0;
    font-size: 14px;
    font-weight: 600;
    color: var(--text-primary);
  }
  .hub-substate {
    font-size: 12px;
    color: var(--text-muted);
  }

  .hub-meta {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .meta-row {
    display: flex;
    align-items: center;
    gap: var(--spacing-md);
    font-size: 12px;
  }
  .meta-label {
    width: 60px;
    color: var(--text-muted);
  }
  .meta-value {
    flex: 1;
    color: var(--text-primary);
    font-family: var(--font-mono);
    word-break: break-all;
  }

  .manage-link {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    margin-top: var(--spacing-sm);
    padding: 0;
    background: transparent;
    border: none;
    color: var(--accent-primary);
    font-size: 12px;
    cursor: pointer;
  }
  .manage-link:hover {
    text-decoration: underline;
  }

  .hub-divider {
    height: 1px;
    background: var(--border-primary);
    margin: var(--spacing-md) 0;
  }

  .hub-activity {
    display: flex;
    flex-direction: column;
    gap: var(--spacing-sm);
  }
  .activity-row {
    display: flex;
    align-items: center;
    gap: var(--spacing-sm);
    font-size: 12px;
    color: var(--text-secondary);
  }
  .activity-row :global(svg) {
    color: var(--text-muted);
    flex-shrink: 0;
  }
  .activity-label {
    flex: 1;
    color: var(--text-muted);
  }
  .activity-value {
    color: var(--text-primary);
    font-variant-numeric: tabular-nums;
  }

  /* Danger zone */
  .danger-zone {
    display: flex;
    justify-content: flex-end;
    margin-top: var(--spacing-md);
  }

  /* Empty state (kept from previous version) */
  .empty {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--spacing-md);
    background: var(--bg-secondary);
    border: 1px dashed var(--border-primary);
    border-radius: 6px;
    color: var(--text-secondary);
    font-size: 13px;
    margin-top: var(--spacing-md);
  }
</style>
