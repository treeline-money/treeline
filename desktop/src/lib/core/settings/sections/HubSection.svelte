<script lang="ts">
  import { onMount } from "svelte";
  import { Icon } from "../../../shared";
  import {
    getHubLinkStatus,
    unlinkHub,
    hubWatch,
    toast,
    type HubLinkStatus,
  } from "../../../sdk";
  import HubLinkModal from "../HubLinkModal.svelte";
  import "../settings-shared.css";

  let status = $state<HubLinkStatus | null>(null);
  let loading = $state(true);
  let showLinkModal = $state(false);
  let unlinking = $state(false);

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

  function formatDate(d: string | null): string {
    if (!d) return "never";
    return new Date(d).toLocaleString();
  }
</script>

<section class="section">
  <h3 class="section-title">Hub</h3>
  <p class="group-desc">
    Link this device to a Treeline hub so your database flows between your devices automatically.
  </p>

  {#if loading}
    <p class="group-desc">Checking…</p>
  {:else if status}
    <div class="setting-row">
      <span class="setting-label">Hub</span>
      <span class="setting-value">{status.url}</span>
    </div>
    <div class="setting-row">
      <span class="setting-label">Device name</span>
      <span class="setting-value">{status.device_name}</span>
    </div>
    <div class="setting-row">
      <span class="setting-label">Last push</span>
      <span class="setting-value">{formatDate(status.last_push)}</span>
    </div>
    <div class="setting-row">
      <span class="setting-label">Last pull</span>
      <span class="setting-value">{formatDate(status.last_pull)}</span>
    </div>
    <div class="setting-row">
      <span class="setting-label">Watcher</span>
      <span class="setting-value">
        {hubWatch.running ? `running (${hubWatch.status})` : "not running"}
      </span>
    </div>

    <div class="actions">
      <button class="btn secondary" onclick={handleUnlink} disabled={unlinking}>
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
  .actions {
    display: flex;
    gap: var(--spacing-sm);
    justify-content: flex-end;
    margin-top: var(--spacing-md);
  }
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
