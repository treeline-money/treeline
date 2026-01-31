<script lang="ts">
  import { Icon } from "../shared";
  import { enableDemo, setAppSetting, toast } from "../sdk";

  interface Props {
    onComplete: (action?: "settings" | "import" | "manual") => void;
  }

  let { onComplete }: Props = $props();

  let isLoading = $state(false);
  let loadingAction = $state<"demo" | "bank" | "import" | "manual" | null>(null);

  async function handleTryDemo() {
    isLoading = true;
    loadingAction = "demo";
    try {
      toast.info("Setting up demo...", "Loading sample data");
      await enableDemo();
      await setAppSetting("hasCompletedOnboarding", true);
      toast.success("Demo mode enabled", "Explore with sample data");
      onComplete();
    } catch (e) {
      toast.error("Failed to enable demo", e instanceof Error ? e.message : String(e));
      isLoading = false;
      loadingAction = null;
    }
  }

  async function handleConnectBank() {
    isLoading = true;
    loadingAction = "bank";
    try {
      await setAppSetting("hasCompletedOnboarding", true);
      onComplete("settings");
    } catch (e) {
      toast.error("Failed to save settings", e instanceof Error ? e.message : String(e));
      isLoading = false;
      loadingAction = null;
    }
  }

  async function handleImportData() {
    isLoading = true;
    loadingAction = "import";
    try {
      await setAppSetting("hasCompletedOnboarding", true);
      onComplete("import");
    } catch (e) {
      toast.error("Failed to save settings", e instanceof Error ? e.message : String(e));
      isLoading = false;
      loadingAction = null;
    }
  }

  async function handleStartManual() {
    isLoading = true;
    loadingAction = "manual";
    try {
      await setAppSetting("hasCompletedOnboarding", true);
      onComplete("manual");
    } catch (e) {
      toast.error("Failed to save settings", e instanceof Error ? e.message : String(e));
      isLoading = false;
      loadingAction = null;
    }
  }
</script>

<div class="welcome-overlay" role="dialog" aria-modal="true" aria-labelledby="welcome-title">
  <div class="welcome-modal">
    <div class="welcome-header">
      <svg class="logo" viewBox="0 0 64 64" fill="none" xmlns="http://www.w3.org/2000/svg">
        <path d="M32 12 L20 35 L35 40 L44 35 Z" fill="var(--logo-snow)"/>
        <path d="M20 35 L35 40 L44 35 L54 52 L10 52 Z" fill="var(--accent-primary)"/>
        <path d="M32 12 L54 52 L10 52 Z" stroke="var(--accent-primary)" stroke-width="2.5" fill="none"/>
      </svg>
      <h1 id="welcome-title" class="brand-name">treeline</h1>
    </div>

    <div class="welcome-content">
      <p class="welcome-prompt">How would you like to get started?</p>
      <div class="options">
        <button
          class="option-card"
          onclick={handleConnectBank}
          disabled={isLoading}
        >
          <div class="option-icon">
            <Icon name="link" size={24} />
          </div>
          <div class="option-content">
            <h3>Connect Bank</h3>
            <p>Automatically sync transactions via SimpleFIN</p>
          </div>
          {#if loadingAction === "bank"}
            <div class="option-loading">
              <div class="spinner"></div>
            </div>
          {:else}
            <Icon name="arrow-right" size={18} />
          {/if}
        </button>

        <button
          class="option-card"
          onclick={handleImportData}
          disabled={isLoading}
        >
          <div class="option-icon">
            <Icon name="database" size={24} />
          </div>
          <div class="option-content">
            <h3>Import Data</h3>
            <p>Import from CSV files or bank statements</p>
          </div>
          {#if loadingAction === "import"}
            <div class="option-loading">
              <div class="spinner"></div>
            </div>
          {:else}
            <Icon name="arrow-right" size={18} />
          {/if}
        </button>

        <button
          class="option-card"
          onclick={handleStartManual}
          disabled={isLoading}
        >
          <div class="option-icon">
            <Icon name="edit" size={24} />
          </div>
          <div class="option-content">
            <h3>Start Manual</h3>
            <p>Add accounts and transactions by hand</p>
          </div>
          {#if loadingAction === "manual"}
            <div class="option-loading">
              <div class="spinner"></div>
            </div>
          {:else}
            <Icon name="arrow-right" size={18} />
          {/if}
        </button>

        <div class="divider"><span>or</span></div>

        <button
          class="option-card demo"
          onclick={handleTryDemo}
          disabled={isLoading}
        >
          <div class="option-icon demo">
            <Icon name="beaker" size={24} />
          </div>
          <div class="option-content">
            <h3>Try Demo Mode</h3>
            <p>Explore with sample data first</p>
          </div>
          {#if loadingAction === "demo"}
            <div class="option-loading">
              <div class="spinner"></div>
            </div>
          {:else}
            <Icon name="arrow-right" size={18} />
          {/if}
        </button>
      </div>

      <div class="welcome-footer">
        <Icon name="package" size={14} />
        <span>Extend Treeline with plugins like budgeting, goals, and more.</span>
      </div>
    </div>
  </div>
</div>

<style>
  .welcome-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.8);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 2000;
    backdrop-filter: blur(4px);
  }

  .welcome-modal {
    background: var(--bg-primary);
    border: 1px solid var(--border-primary);
    border-radius: 16px;
    width: 90%;
    max-width: 520px;
    box-shadow: 0 24px 64px rgba(0, 0, 0, 0.5);
    overflow: hidden;
  }

  .welcome-header {
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
    padding: var(--spacing-xl) var(--spacing-xl) var(--spacing-lg);
    background: var(--bg-secondary);
    border-bottom: 1px solid var(--border-primary);
  }

  .logo {
    width: 64px;
    height: 64px;
    margin-bottom: var(--spacing-sm);
  }

  .brand-name {
    margin: 0;
    font-family: 'Outfit', var(--font-sans);
    font-size: 32px;
    font-weight: 600;
    color: var(--text-primary);
    letter-spacing: -0.5px;
  }

  .welcome-content {
    padding: var(--spacing-lg) var(--spacing-xl);
  }

  .welcome-prompt {
    margin: 0 0 var(--spacing-md) 0;
    font-size: 14px;
    color: var(--text-secondary);
    text-align: center;
  }

  .options {
    display: flex;
    flex-direction: column;
    gap: var(--spacing-sm);
  }

  .option-card {
    display: flex;
    align-items: center;
    gap: var(--spacing-md);
    padding: var(--spacing-sm) var(--spacing-md);
    background: var(--bg-secondary);
    border: 1px solid var(--border-primary);
    border-radius: 8px;
    cursor: pointer;
    transition: all 0.2s ease;
    text-align: left;
    color: var(--text-primary);
  }

  .option-card:hover:not(:disabled) {
    border-color: var(--accent-primary);
    background: var(--bg-tertiary);
  }

  .option-card:disabled {
    opacity: 0.7;
    cursor: not-allowed;
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
    transition: transform 0.2s ease;
  }

  .option-card:hover:not(:disabled) :global(svg) {
    color: var(--accent-primary);
    transform: translateX(4px);
  }

  .option-loading {
    width: 18px;
    height: 18px;
    flex-shrink: 0;
  }

  .spinner {
    width: 18px;
    height: 18px;
    border: 2px solid var(--border-primary);
    border-top-color: var(--accent-primary);
    border-radius: 50%;
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  .divider {
    display: flex;
    align-items: center;
    gap: var(--spacing-md);
    color: var(--text-muted);
    font-size: 12px;
    margin: var(--spacing-xs) 0;
  }

  .divider::before,
  .divider::after {
    content: "";
    flex: 1;
    height: 1px;
    background: var(--border-primary);
  }

  .option-card.demo {
    background: transparent;
    border-style: dashed;
  }

  .option-card.demo:hover:not(:disabled) {
    background: var(--bg-secondary);
  }

  .option-icon.demo {
    background: transparent;
    color: var(--text-muted);
  }

  .option-card.demo:hover:not(:disabled) .option-icon.demo {
    color: var(--accent-primary);
  }

  .welcome-footer {
    display: flex;
    align-items: center;
    gap: var(--spacing-sm);
    margin-top: var(--spacing-lg);
    padding: var(--spacing-sm) var(--spacing-md);
    background: var(--bg-tertiary);
    border-radius: 6px;
    font-size: 12px;
    color: var(--text-muted);
  }

  .welcome-footer :global(svg) {
    flex-shrink: 0;
    color: var(--accent-primary);
  }
</style>
