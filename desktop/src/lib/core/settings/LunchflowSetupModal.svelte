<script lang="ts">
  import { Icon, LUNCHFLOW } from "../../shared";
  import "./settings-shared.css";

  interface SetupAccount {
    lunchflow_id: string;
    name: string;
    institution_name: string | null;
    balance: string | null;
    currency: string | null;
    balances_only: boolean;
  }

  interface Props {
    isOpen: boolean;
    apiKey: string;
    isSettingUp: boolean;
    setupError: string | null;
    setupSuccess: boolean;
    isFetchingAccounts: boolean;
    setupAccounts: SetupAccount[];
    onClose: () => void;
    onApiKeyChange: (key: string) => void;
    onSetup: () => void;
    onToggleAccountBalancesOnly: (lunchflowId: string) => void;
    onSyncAfterSetup: () => void;
    onOpenExternalUrl: (url: string) => void;
  }

  let {
    isOpen,
    apiKey,
    isSettingUp,
    setupError,
    setupSuccess,
    isFetchingAccounts,
    setupAccounts,
    onClose,
    onApiKeyChange,
    onSetup,
    onToggleAccountBalancesOnly,
    onSyncAfterSetup,
    onOpenExternalUrl,
  }: Props = $props();

  function formatBalance(balance: string | null, currency: string | null): string {
    if (!balance) return "";
    const num = parseFloat(balance);
    const curr = currency || "USD";
    try {
      return new Intl.NumberFormat(undefined, {
        style: "currency",
        currency: curr,
      }).format(num);
    } catch {
      return `${curr} ${num.toFixed(2)}`;
    }
  }
</script>

{#if isOpen}
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div class="sub-modal-overlay" onclick={onClose} onkeydown={(e) => e.key === 'Escape' && onClose()} role="dialog" aria-modal="true" tabindex="-1">
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <div class="sub-modal" role="document" onclick={(e) => e.stopPropagation()}>
      {#if setupSuccess}
        <div class="sub-modal-header">
          <span class="sub-modal-title">{LUNCHFLOW.name} Connected</span>
          <button class="close-btn" onclick={onClose}>
            <Icon name="x" size={16} />
          </button>
        </div>
        <div class="sub-modal-body setup-accounts-body">
          {#if isFetchingAccounts}
            <div class="fetching-accounts">
              <div class="spinner"></div>
              <p>Fetching your accounts...</p>
            </div>
          {:else if setupAccounts.length > 0}
            <div class="setup-accounts-intro">
              <div class="success-icon small">
                <Icon name="check" size={20} />
              </div>
              <p>Found {setupAccounts.length} account{setupAccounts.length > 1 ? 's' : ''}. Choose what to sync for each:</p>
            </div>
            <div class="setup-accounts-list">
              {#each setupAccounts as account (account.lunchflow_id)}
                <div class="setup-account-item">
                  <div class="setup-account-info">
                    <span class="setup-account-name">{account.name}</span>
                    {#if account.institution_name}
                      <span class="setup-account-institution">{account.institution_name}</span>
                    {/if}
                    {#if account.balance}
                      <span class="setup-account-balance">{formatBalance(account.balance, account.currency)}</span>
                    {/if}
                  </div>
                  <div class="setup-account-toggle">
                    <button
                      class="toggle-option"
                      class:active={!account.balances_only}
                      onclick={() => { if (account.balances_only) onToggleAccountBalancesOnly(account.lunchflow_id); }}
                    >
                      Balances + Transactions
                    </button>
                    <button
                      class="toggle-option"
                      class:active={account.balances_only}
                      onclick={() => { if (!account.balances_only) onToggleAccountBalancesOnly(account.lunchflow_id); }}
                    >
                      Balances only
                    </button>
                  </div>
                </div>
              {/each}
            </div>
            <p class="setup-accounts-hint">
              "Balances + Transactions" syncs balances and 90 days of transactions. "Balances only" skips transaction history.
            </p>
          {:else}
            <div class="setup-accounts-intro">
              <div class="success-icon small">
                <Icon name="check" size={20} />
              </div>
              <p>{LUNCHFLOW.name} connected! Click Start Syncing to fetch your accounts.</p>
            </div>
          {/if}
        </div>
        <div class="sub-modal-actions">
          <button class="btn secondary" onclick={onClose}>Close</button>
          <button class="btn primary" onclick={onSyncAfterSetup} disabled={isFetchingAccounts}>
            {isFetchingAccounts ? "Loading..." : "Start Syncing"}
          </button>
        </div>
      {:else if isSettingUp}
        <div class="sub-modal-body loading-body">
          <div class="spinner"></div>
          <p class="loading-text">Connecting to {LUNCHFLOW.name}...</p>
          <p class="loading-hint">Validating your API key</p>
        </div>
      {:else}
        <div class="sub-modal-header">
          <span class="sub-modal-title">Connect {LUNCHFLOW.name}</span>
          <button class="close-btn" onclick={onClose}>
            <Icon name="x" size={16} />
          </button>
        </div>
        <div class="sub-modal-body">
          {#if setupError}
            <div class="setup-error">
              <strong>Connection Failed</strong>
              <p>{setupError}</p>
            </div>
          {/if}

          <div class="lunchflow-intro">
            <p>{LUNCHFLOW.name} connects to <strong>{LUNCHFLOW.banks} banks</strong> across {LUNCHFLOW.coverage}.</p>
          </div>

          <div class="setup-steps">
            <div class="step">
              <span class="step-num">1</span>
              <div class="step-content">
                <span>Sign up at {LUNCHFLOW.name}:</span>
                <button
                  class="link-btn inline"
                  onclick={() => onOpenExternalUrl(LUNCHFLOW.url)}
                >
                  lunchflow.app
                  <Icon name="external-link" size={12} />
                </button>
              </div>
            </div>
            <div class="step">
              <span class="step-num">2</span>
              <span class="step-content">Connect your banks under "Connections"</span>
            </div>
            <div class="step">
              <span class="step-num">3</span>
              <span class="step-content">Go to "Destinations" and create an API destination</span>
            </div>
            <div class="step">
              <span class="step-num">4</span>
              <span class="step-content">Copy your API key and paste it below</span>
            </div>
          </div>

          <div class="token-input-group">
            <label for="api-key">API Key</label>
            <input
              id="api-key"
              type="text"
              value={apiKey}
              oninput={(e) => onApiKeyChange(e.currentTarget.value)}
              placeholder="Paste your API key here"
              class="token-input"
            />
            <span class="token-hint">Your {LUNCHFLOW.name} API key from the Destinations page</span>
          </div>

          <div class="pricing-note">
            <Icon name="info" size={14} />
            <span>{LUNCHFLOW.name} costs {LUNCHFLOW.pricing}. This is paid directly to {LUNCHFLOW.name}.</span>
          </div>
        </div>
        <div class="sub-modal-actions">
          <button class="btn secondary" onclick={onClose}>Cancel</button>
          <button class="btn primary" onclick={onSetup} disabled={!apiKey.trim()}>
            Connect
          </button>
        </div>
      {/if}
    </div>
  </div>
{/if}

<style>
  /* Lunchflow intro */
  .lunchflow-intro {
    margin-bottom: var(--spacing-md);
    padding: var(--spacing-sm) var(--spacing-md);
    background: rgba(59, 130, 246, 0.1);
    border: 1px solid rgba(59, 130, 246, 0.2);
    border-radius: 6px;
  }

  .lunchflow-intro p {
    margin: 0;
    font-size: 12px;
    color: var(--text-primary);
    line-height: 1.5;
  }

  /* Setup error */
  .setup-error {
    background: rgba(239, 68, 68, 0.15);
    border: 1px solid rgba(239, 68, 68, 0.3);
    border-radius: 6px;
    padding: var(--spacing-sm) var(--spacing-md);
    margin-bottom: var(--spacing-md);
    color: var(--text-negative, #ef4444);
    font-size: 12px;
  }

  .setup-error strong {
    display: block;
    margin-bottom: 4px;
  }

  .setup-error p {
    margin: 0;
    opacity: 0.9;
  }

  /* Setup steps */
  .setup-steps {
    margin-bottom: var(--spacing-md);
  }

  .step {
    display: flex;
    gap: var(--spacing-sm);
    margin-bottom: var(--spacing-sm);
  }

  .step:last-child {
    margin-bottom: 0;
  }

  .step-num {
    width: 20px;
    height: 20px;
    background: var(--bg-tertiary);
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 11px;
    font-weight: 600;
    color: var(--text-secondary);
    flex-shrink: 0;
  }

  .step-content {
    font-size: 12px;
    color: var(--text-primary);
    line-height: 1.5;
    padding-top: 2px;
  }

  /* Token input */
  .token-input-group {
    background: var(--bg-secondary);
    border: 1px solid var(--border-primary);
    border-radius: 6px;
    padding: var(--spacing-sm) var(--spacing-md);
    margin-bottom: var(--spacing-md);
  }

  .token-input-group label {
    display: block;
    font-size: 11px;
    font-weight: 500;
    color: var(--text-secondary);
    margin-bottom: 4px;
  }

  .token-input {
    width: 100%;
    padding: 8px 10px;
    background: var(--bg-primary);
    border: 1px solid var(--border-primary);
    border-radius: 4px;
    color: var(--text-primary);
    font-size: 12px;
    font-family: var(--font-mono);
  }

  .token-input:focus {
    outline: none;
    border-color: var(--accent-primary);
  }

  .token-hint {
    display: block;
    font-size: 10px;
    color: var(--text-muted);
    margin-top: 4px;
  }

  /* Pricing note */
  .pricing-note {
    display: flex;
    align-items: flex-start;
    gap: var(--spacing-xs);
    font-size: 11px;
    color: var(--text-muted);
    padding: var(--spacing-sm);
    background: var(--bg-secondary);
    border-radius: 4px;
  }

  .pricing-note :global(svg) {
    flex-shrink: 0;
    margin-top: 1px;
  }

  /* Loading state */
  .loading-body {
    display: flex;
    flex-direction: column;
    align-items: center;
    padding: var(--spacing-xl) var(--spacing-lg);
    gap: var(--spacing-sm);
  }

  .loading-text {
    font-size: 13px;
    color: var(--text-primary);
    margin: 0;
  }

  .loading-hint {
    font-size: 11px;
    color: var(--text-muted);
    margin: 0;
  }

  /* Success state */
  .success-icon {
    width: 48px;
    height: 48px;
    background: var(--accent-success, #22c55e);
    color: white;
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .success-icon.small {
    width: 28px;
    height: 28px;
    padding: 4px;
  }

  /* Setup accounts */
  .setup-accounts-body {
    padding: var(--spacing-lg);
    max-height: 400px;
    overflow-y: auto;
  }

  .fetching-accounts {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--spacing-md);
    padding: var(--spacing-xl);
    color: var(--text-muted);
  }

  .setup-accounts-intro {
    display: flex;
    align-items: center;
    gap: var(--spacing-sm);
    margin-bottom: var(--spacing-md);
  }

  .setup-accounts-intro p {
    margin: 0;
    font-size: 13px;
    color: var(--text-primary);
  }

  .setup-accounts-list {
    display: flex;
    flex-direction: column;
    gap: var(--spacing-xs);
    margin-bottom: var(--spacing-md);
  }

  .setup-account-item {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: var(--spacing-sm) var(--spacing-md);
    background: var(--bg-primary);
    border: 1px solid var(--border-primary);
    border-radius: 6px;
    gap: var(--spacing-md);
  }

  .setup-account-info {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
    flex: 1;
  }

  .setup-account-name {
    font-size: 13px;
    font-weight: 500;
    color: var(--text-primary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .setup-account-institution {
    font-size: 11px;
    color: var(--text-muted);
  }

  .setup-account-balance {
    font-size: 12px;
    font-weight: 500;
    color: var(--text-secondary);
    font-family: var(--font-mono);
  }

  .setup-account-toggle {
    display: flex;
    gap: 1px;
    background: var(--bg-tertiary);
    border-radius: 4px;
    overflow: hidden;
    flex-shrink: 0;
  }

  .setup-account-toggle .toggle-option {
    padding: 4px 10px;
    font-size: 11px;
    border: none;
    background: transparent;
    color: var(--text-muted);
    cursor: pointer;
    transition: all 0.15s;
  }

  .setup-account-toggle .toggle-option:hover {
    color: var(--text-primary);
  }

  .setup-account-toggle .toggle-option.active {
    background: var(--accent-primary);
    color: white;
  }

  .setup-accounts-hint {
    font-size: 11px;
    color: var(--text-muted);
    margin: 0;
    text-align: center;
  }
</style>
