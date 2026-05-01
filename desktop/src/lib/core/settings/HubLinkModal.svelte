<script lang="ts">
  import { Icon } from "../../shared";
  import {
    startHubLink,
    pollHubLink,
    cancelHubLink,
    type HubLinkInfo,
    hubWatch,
    toast,
  } from "../../sdk";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import "./settings-shared.css";

  interface Props {
    isOpen: boolean;
    /** Called after a successful link. Parent should refresh status. */
    onLinked: () => void;
    onClose: () => void;
  }

  let { isOpen, onLinked, onClose }: Props = $props();

  type Phase = "form" | "waiting" | "error";

  let phase = $state<Phase>("form");
  let url = $state("https://pro.treeline.money");
  let deviceName = $state("");
  let info = $state<HubLinkInfo | null>(null);
  let errorMessage = $state<string | null>(null);
  let pollHandle: ReturnType<typeof setTimeout> | null = null;

  $effect(() => {
    if (!isOpen) {
      stopPolling();
      phase = "form";
      info = null;
      errorMessage = null;
    }
  });

  function stopPolling() {
    if (pollHandle) {
      clearTimeout(pollHandle);
      pollHandle = null;
    }
  }

  async function handleStart() {
    errorMessage = null;
    try {
      const result = await startHubLink(url.trim(), deviceName.trim() || "Treeline desktop");
      info = result;
      phase = "waiting";
      schedulePoll(result.interval);
    } catch (e) {
      errorMessage = String(e);
      phase = "error";
    }
  }

  function schedulePoll(intervalSeconds: number) {
    pollHandle = setTimeout(async () => {
      try {
        const result = await pollHubLink();
        switch (result.status) {
          case "pending":
            schedulePoll(intervalSeconds);
            break;
          case "slow_down":
            schedulePoll(intervalSeconds * 2);
            break;
          case "linked":
            stopPolling();
            toast.success(`Linked to ${result.hub_url} as "${result.device_name}"`);
            await hubWatch.start();
            onLinked();
            onClose();
            break;
          case "expired":
            stopPolling();
            errorMessage = "The link request expired before authorization completed.";
            phase = "error";
            break;
          case "denied":
            stopPolling();
            errorMessage = "Authorization was denied.";
            phase = "error";
            break;
        }
      } catch (e) {
        stopPolling();
        errorMessage = String(e);
        phase = "error";
      }
    }, intervalSeconds * 1000);
  }

  async function handleClose() {
    stopPolling();
    await cancelHubLink().catch(() => {});
    onClose();
  }

  async function copy(text: string) {
    await navigator.clipboard.writeText(text);
    toast.info("Copied");
  }
</script>

{#if isOpen}
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div
    class="sub-modal-overlay"
    onclick={handleClose}
    onkeydown={(e) => e.key === "Escape" && handleClose()}
    role="dialog"
    aria-modal="true"
    tabindex="-1"
  >
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="sub-modal" onclick={(e) => e.stopPropagation()}>
      <div class="sub-modal-header">
        <span class="sub-modal-title">Link to a hub</span>
        <button class="close-btn" onclick={handleClose} aria-label="Close">
          <Icon name="x" size={18} />
        </button>
      </div>

      <div class="sub-modal-body">
        {#if phase === "form"}
          <p class="group-desc">
            Linking lets this device push and pull its database against a Treeline hub.
            The default is Treeline Cloud — replace the URL to point at a self-hosted hub.
          </p>

          <div class="form-group">
            <label for="hub-link-url">Hub URL</label>
            <input
              id="hub-link-url"
              type="url"
              bind:value={url}
              placeholder="https://pro.treeline.money"
              autocomplete="off"
            />
          </div>

          <div class="form-group">
            <label for="hub-link-name">Device name</label>
            <input
              id="hub-link-name"
              type="text"
              bind:value={deviceName}
              placeholder="this Mac"
              autocomplete="off"
            />
          </div>
        {:else if phase === "waiting" && info}
          <p class="group-desc">
            Open this URL in your browser to authorize this device. We'll detect the
            authorization automatically — no need to come back to this window.
          </p>

          <div class="link-info">
            <div class="link-row">
              <span class="link-label">URL</span>
              <code class="link-value">{info.verification_uri_complete}</code>
              <button
                class="link-action"
                title="Copy URL"
                onclick={() => copy(info!.verification_uri_complete)}
                aria-label="Copy URL"
              >
                <Icon name="copy" size={14} />
              </button>
              <button
                class="link-action"
                title="Open in browser"
                onclick={() => openUrl(info!.verification_uri_complete)}
                aria-label="Open in browser"
              >
                <Icon name="external-link" size={14} />
              </button>
            </div>
            {#if info.user_code}
              <div class="link-row">
                <span class="link-label">Code</span>
                <code class="link-value link-code">{info.user_code}</code>
                <button
                  class="link-action"
                  title="Copy code"
                  onclick={() => copy(info!.user_code)}
                  aria-label="Copy code"
                >
                  <Icon name="copy" size={14} />
                </button>
              </div>
            {/if}
          </div>

          <div class="waiting-state">
            <div class="spinner"></div>
            <span>Waiting for authorization…</span>
          </div>
        {:else if phase === "error"}
          <p class="error-text">{errorMessage}</p>
        {/if}
      </div>

      <div class="sub-modal-actions">
        {#if phase === "form"}
          <button class="btn secondary" onclick={handleClose}>Cancel</button>
          <button class="btn primary" onclick={handleStart} disabled={!url.trim()}>
            Continue
          </button>
        {:else if phase === "waiting"}
          <button class="btn secondary" onclick={handleClose}>Cancel</button>
        {:else if phase === "error"}
          <button class="btn secondary" onclick={handleClose}>Close</button>
          <button
            class="btn primary"
            onclick={() => {
              phase = "form";
              errorMessage = null;
            }}>Try again</button
          >
        {/if}
      </div>
    </div>
  </div>
{/if}

<style>
  .link-info {
    background: var(--bg-secondary);
    border: 1px solid var(--border-primary);
    border-radius: 6px;
    padding: var(--spacing-md);
    margin-bottom: var(--spacing-md);
    display: flex;
    flex-direction: column;
    gap: var(--spacing-sm);
  }
  .link-row {
    display: flex;
    align-items: center;
    gap: var(--spacing-sm);
  }
  .link-label {
    width: 40px;
    font-size: 11px;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }
  .link-value {
    flex: 1;
    font-family: var(--font-mono);
    font-size: 13px;
    color: var(--text-primary);
    word-break: break-all;
  }
  .link-code {
    /* Match the URL row's font-family/size; the label is enough to
       distinguish the two rows. Custom letter-spacing or bold makes the
       monospace fonts look inconsistent. */
  }
  .link-action {
    background: transparent;
    border: 1px solid var(--border-primary);
    border-radius: 4px;
    color: var(--text-muted);
    cursor: pointer;
    padding: 4px 6px;
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .link-action:hover {
    background: var(--bg-tertiary);
    color: var(--text-primary);
  }
  .waiting-state {
    display: flex;
    align-items: center;
    gap: var(--spacing-sm);
    color: var(--text-secondary);
    font-size: 13px;
  }
  .waiting-state .spinner {
    width: 16px;
    height: 16px;
    border-width: 2px;
  }
  .error-text {
    color: var(--accent-danger, #dc2626);
    font-size: 13px;
    margin: 0;
  }
</style>
