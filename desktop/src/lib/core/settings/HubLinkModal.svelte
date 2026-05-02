<script lang="ts">
  import { Icon } from "../../shared";
  import {
    startHubLink,
    pollHubLink,
    cancelHubLink,
    pushToHubNow,
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

  type Destination = "cloud" | "self-hosted";
  type Phase = "choose" | "form" | "waiting" | "error";

  const CLOUD_URL = "https://pro.treeline.money";
  const DEFAULT_NAME = "Treeline desktop";

  let phase = $state<Phase>("choose");
  let destination = $state<Destination>("cloud");
  let url = $state(CLOUD_URL);
  let deviceName = $state(DEFAULT_NAME);
  let info = $state<HubLinkInfo | null>(null);
  let errorMessage = $state<string | null>(null);
  let pollHandle: ReturnType<typeof setTimeout> | null = null;

  $effect(() => {
    if (!isOpen) {
      stopPolling();
      phase = "choose";
      destination = "cloud";
      url = CLOUD_URL;
      deviceName = DEFAULT_NAME;
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

  function pickDestination(choice: Destination) {
    destination = choice;
    if (choice === "cloud") {
      url = CLOUD_URL;
    } else {
      url = "";
    }
    phase = "form";
  }

  async function handleStart() {
    errorMessage = null;
    try {
      const result = await startHubLink(url.trim(), deviceName.trim() || DEFAULT_NAME);
      info = result;
      phase = "waiting";
      // Auto-open the verification URL: that's the next step regardless of
      // destination, and skipping the click is the whole UX win here.
      openUrl(result.verification_uri_complete).catch((e) =>
        console.warn("[hub-link] auto-open failed:", e),
      );
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
            // Initial push: the device is the source of truth post-link, so
            // get the local DB onto the hub before the watch loop kicks in
            // (which would otherwise pull and clobber a fresh link with hub
            // data that may have come from elsewhere).
            try {
              const pushResult = await pushToHubNow();
              switch (pushResult.status) {
                case "pushed":
                case "auto_merged":
                case "no_changes":
                  toast.success(`Linked as "${result.device_name}"`);
                  break;
                case "conflict":
                case "no_base_snapshot":
                  toast.warning(
                    "Linked, but the hub has data that conflicts with this device. Resolve via CLI: tl hub push --force or tl hub pull.",
                  );
                  break;
              }
            } catch (e) {
              toast.warning(
                `Linked as "${result.device_name}", but the initial push failed: ${e}`,
              );
            }
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
        {#if phase === "choose"}
          <p class="group-desc">Where would you like to link this device?</p>

          <div class="options">
            <button class="option-card option-cloud" onclick={() => pickDestination("cloud")}>
              <div class="option-icon">
                <Icon name="repeat" size={20} />
              </div>
              <div class="option-content">
                <h3>Treeline Cloud</h3>
                <p>Sign in at pro.treeline.money.</p>
              </div>
              <Icon name="arrow-right" size={16} />
            </button>

            <button class="option-card" onclick={() => pickDestination("self-hosted")}>
              <div class="option-icon">
                <Icon name="command" size={20} />
              </div>
              <div class="option-content">
                <h3>Self-hosted hub</h3>
                <p>Connect to a hub you run yourself.</p>
              </div>
              <Icon name="arrow-right" size={16} />
            </button>
          </div>
        {:else if phase === "form"}
          {#if destination === "cloud"}
            <p class="group-desc">
              We'll open <code>pro.treeline.money</code> in your browser. Sign in (or sign
              up) and authorize this device. If it's your first time, you'll need to
              activate a hub from the Pro dashboard before authorizing.
            </p>
          {:else}
            <p class="group-desc">
              Enter the URL of your self-hosted Treeline hub.
            </p>
          {/if}

          {#if destination === "self-hosted"}
            <div class="form-group">
              <label for="hub-link-url">Hub URL</label>
              <input
                id="hub-link-url"
                type="url"
                bind:value={url}
                placeholder="https://your-hub.example.com"
                autocomplete="off"
              />
            </div>
          {/if}

          <div class="form-group">
            <label for="hub-link-name">Device name</label>
            <input
              id="hub-link-name"
              type="text"
              bind:value={deviceName}
              placeholder={DEFAULT_NAME}
              autocomplete="off"
            />
          </div>
        {:else if phase === "waiting" && info}
          <p class="group-desc">
            {#if destination === "cloud"}
              We've opened Treeline Cloud in your browser. Sign in (or sign up) to authorize
              this device. We'll detect the authorization automatically.
            {:else}
              Open this URL in your browser to authorize this device. We'll detect the
              authorization automatically.
            {/if}
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
                <code class="link-value">{info.user_code}</code>
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
          {#if destination === "cloud"}
            <p class="group-desc">
              Confirm you've signed in at <code>pro.treeline.money</code> and activated a
              hub from the dashboard, then try again.
            </p>
            <div class="error-action">
              <button class="btn secondary" onclick={() => openUrl(CLOUD_URL)}>
                <Icon name="external-link" size={14} />
                Open Treeline Cloud
              </button>
            </div>
          {/if}
        {/if}
      </div>

      <div class="sub-modal-actions">
        {#if phase === "choose"}
          <button class="btn secondary" onclick={handleClose}>Cancel</button>
        {:else if phase === "form"}
          <button class="btn secondary" onclick={() => (phase = "choose")}>Back</button>
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
              phase = destination === "self-hosted" ? "form" : "choose";
              errorMessage = null;
            }}>Try again</button
          >
        {/if}
      </div>
    </div>
  </div>
{/if}

<style>
  .options {
    display: flex;
    flex-direction: column;
    gap: var(--spacing-sm);
  }
  .option-card {
    display: flex;
    align-items: center;
    gap: var(--spacing-md);
    padding: var(--spacing-md);
    background: var(--bg-secondary);
    border: 1px solid var(--border-primary);
    border-radius: 8px;
    cursor: pointer;
    transition:
      border-color 0.15s ease,
      background 0.15s ease;
    text-align: left;
    color: var(--text-primary);
    width: 100%;
  }
  .option-card:hover {
    border-color: var(--accent-primary);
    background: var(--bg-tertiary);
  }
  .option-cloud {
    border-color: rgba(34, 197, 94, 0.4);
  }
  .option-icon {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 40px;
    height: 40px;
    flex-shrink: 0;
    background: var(--bg-tertiary);
    border-radius: 8px;
    color: var(--accent-primary);
  }
  .option-cloud .option-icon {
    background: rgba(34, 197, 94, 0.12);
  }
  .option-content {
    flex: 1;
    min-width: 0;
  }
  .option-content h3 {
    margin: 0 0 2px 0;
    font-size: 14px;
    font-weight: 600;
    color: var(--text-primary);
  }
  .option-content p {
    margin: 0;
    font-size: 12px;
    color: var(--text-muted);
    line-height: 1.3;
  }
  .option-card :global(svg) {
    color: var(--text-muted);
    flex-shrink: 0;
    transition: transform 0.15s ease;
  }
  .option-card:hover :global(svg) {
    color: var(--accent-primary);
    transform: translateX(3px);
  }

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
    margin: 0 0 var(--spacing-md) 0;
  }
  .error-action {
    margin-top: var(--spacing-md);
  }

  code {
    font-family: var(--font-mono);
    font-size: 12px;
    padding: 1px 4px;
    background: var(--bg-tertiary);
    border-radius: 3px;
    color: var(--text-secondary);
  }
</style>
