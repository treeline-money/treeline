<script lang="ts">
  import { Icon, SIMPLEFIN, LUNCHFLOW } from "../../../shared";
  import type { Settings } from "../../../sdk";
  import "../settings-shared.css";

  interface SimplefinAccount {
    account_id: string;
    simplefin_id: string;
    name: string;
    institution_name: string;
    account_type: string | null;
    balances_only: boolean;
  }

  interface LunchflowAccount {
    account_id: string;
    lunchflow_id: string;
    name: string;
    institution_name: string;
    account_type: string | null;
    currency: string | null;
    balances_only: boolean;
  }

  interface SetupAccount {
    simplefin_id: string;
    name: string;
    institution_name: string | null;
    balance: string | null;
    balances_only: boolean;
  }

  interface Integration {
    integration_name: string;
    created_at: string;
    updated_at: string;
  }

  interface Props {
    settings: Settings;
    isDemoMode: boolean;
    isExitingDemo: boolean;
    isSyncing: boolean;
    integrations: Integration[];
    simplefinAccounts: SimplefinAccount[];
    lunchflowAccounts: LunchflowAccount[];
    connectionWarnings: string[];
    isCheckingConnection: boolean;
    connectionCheckSuccess: boolean | null;
    onExitDemoMode: () => void;
    onCheckConnection: () => void;
    onToggleBalancesOnly: (account: SimplefinAccount) => void;
    onToggleLunchflowBalancesOnly: (account: LunchflowAccount) => void;
    onOpenSetupModal: () => void;
    onOpenLunchflowSetupModal: () => void;
    onDisconnect: (integrationName: string) => void;
    onOpenExternalUrl: (url: string) => void;
    formatLastSync: (dateStr: string | null) => string;
  }

  let {
    settings,
    isDemoMode,
    isExitingDemo,
    isSyncing,
    integrations,
    simplefinAccounts,
    lunchflowAccounts = [],
    connectionWarnings,
    isCheckingConnection,
    connectionCheckSuccess,
    onExitDemoMode,
    onCheckConnection,
    onToggleBalancesOnly,
    onToggleLunchflowBalancesOnly,
    onOpenSetupModal,
    onOpenLunchflowSetupModal,
    onDisconnect,
    onOpenExternalUrl,
    formatLastSync,
  }: Props = $props();

  // Sub-modal state
  let showDisconnectConfirm = $state(false);
  let disconnectingIntegration = $state<string | null>(null);

  function openDisconnectConfirm(integrationName: string) {
    disconnectingIntegration = integrationName;
    showDisconnectConfirm = true;
  }

  function closeDisconnectConfirm() {
    showDisconnectConfirm = false;
    disconnectingIntegration = null;
  }

  function handleDisconnect() {
    if (disconnectingIntegration) {
      onDisconnect(disconnectingIntegration);
      closeDisconnectConfirm();
    }
  }

  let isSimplefinConnected = $derived(
    integrations.some((i) => i.integration_name === "simplefin")
  );

  let simplefinIntegration = $derived(
    integrations.find((i) => i.integration_name === "simplefin")
  );

  let isLunchflowConnected = $derived(
    integrations.some((i) => i.integration_name === "lunchflow")
  );

  let lunchflowIntegration = $derived(
    integrations.find((i) => i.integration_name === "lunchflow")
  );

  let accountsByInstitution = $derived.by(() => {
    const groups = new Map<string, SimplefinAccount[]>();
    for (const account of simplefinAccounts) {
      const inst = account.institution_name || "Unknown";
      if (!groups.has(inst)) {
        groups.set(inst, []);
      }
      groups.get(inst)!.push(account);
    }
    return groups;
  });

  let lunchflowAccountsByInstitution = $derived.by(() => {
    const groups = new Map<string, LunchflowAccount[]>();
    for (const account of lunchflowAccounts) {
      const inst = account.institution_name || "Unknown";
      if (!groups.has(inst)) {
        groups.set(inst, []);
      }
      groups.get(inst)!.push(account);
    }
    return groups;
  });
</script>

<section class="section">
  <h3 class="section-title">Integrations</h3>

  {#if isDemoMode}
    <div class="demo-mode-notice">
      <div class="notice-icon"><Icon name="beaker" size={24} /></div>
      <div class="notice-content">
        <h4>Demo Mode Active</h4>
        <p>Integration setup is disabled while in demo mode to prevent mixing real data with sample data.</p>
        <button
          class="btn primary"
          onclick={onExitDemoMode}
          disabled={isExitingDemo}
        >
          {isExitingDemo ? "Exiting..." : "Exit Demo Mode"}
        </button>
      </div>
    </div>
  {:else}
    <div class="integration-card">
      <div class="integration-header">
        <div class="integration-info">
          <div class="integration-title-row">
            <h4 class="integration-name">{SIMPLEFIN.name}</h4>
            <button
              class="link-btn learn-more"
              onclick={() => onOpenExternalUrl(SIMPLEFIN.url)}
            >
              Learn more
              <Icon name="external-link" size={10} />
            </button>
          </div>
          <p class="integration-desc">{SIMPLEFIN.description}</p>
        </div>
        {#if isSimplefinConnected}
          <button class="btn secondary small" onclick={() => openDisconnectConfirm("simplefin")}>
            Disconnect
          </button>
        {/if}
      </div>

      {#if isSimplefinConnected && simplefinIntegration}
        {#if connectionCheckSuccess === false}
          <div class="integration-status warning">
            <span class="status-dot"></span>
            <span>Connection Issue</span>
          </div>
        {:else}
          <div class="integration-status connected">
            <span class="status-dot"></span>
            <span>Connected</span>
          </div>
        {/if}

        {#if simplefinAccounts.length > 0}
          <div class="linked-accounts">
            <div class="accounts-header">
              <span class="accounts-title">Linked Accounts ({simplefinAccounts.length})</span>
              <button
                class="btn secondary small"
                onclick={onCheckConnection}
                disabled={isCheckingConnection}
              >
                {#if isCheckingConnection}
                  Checking...
                {:else}
                  Check Connection
                {/if}
              </button>
            </div>
            <div class="sync-settings-help">
              <Icon name="info" size={14} />
              <span class="help-text">Choose what to sync for each account. Select "Balances only" for accounts where you don't need individual transactions.</span>
            </div>
            {#each [...accountsByInstitution] as [institution, accounts]}
              {@const hasWarning = connectionWarnings.some(w => w.includes(institution))}
              {@const isCheckedOk = connectionCheckSuccess !== null && !hasWarning}
              <div class="institution-group" class:has-warning={hasWarning} class:checked-ok={isCheckedOk}>
                <div class="institution-header">
                  <span class="institution-name">{institution}</span>
                  {#if isCheckingConnection}
                    <span class="institution-status checking">...</span>
                  {:else if connectionCheckSuccess !== null}
                    {#if hasWarning}
                      <span class="institution-status warning">!</span>
                    {:else}
                      <Icon name="check" size={12} class="status-ok" />
                    {/if}
                  {/if}
                </div>
                <div class="institution-accounts">
                  {#each accounts as account}
                    <div class="account-item">
                      <div class="account-info">
                        <span class="account-name">{account.name}</span>
                        {#if account.account_type}
                          <span class="account-type">{account.account_type}</span>
                        {/if}
                      </div>
                      <div class="segmented-toggle">
                        <button
                          class="toggle-option"
                          class:active={!account.balances_only}
                          onclick={() => { if (account.balances_only) onToggleBalancesOnly(account); }}
                        >
                          Balances + Transactions
                        </button>
                        <button
                          class="toggle-option"
                          class:active={account.balances_only}
                          onclick={() => { if (!account.balances_only) onToggleBalancesOnly(account); }}
                        >
                          Balances only
                        </button>
                      </div>
                    </div>
                  {/each}
                </div>
              </div>
            {/each}

            {#if connectionWarnings.length > 0}
              <div class="connection-warnings">
                {#each connectionWarnings as warning}
                  <div class="warning-item">
                    <span class="warning-icon">!</span>
                    <span class="warning-text">{warning}</span>
                  </div>
                {/each}
                <button
                  class="link-btn"
                  onclick={() => onOpenExternalUrl(SIMPLEFIN.url)}
                >
                  Fix connection issues on SimpleFIN
                  <Icon name="external-link" size={12} />
                </button>
              </div>
            {/if}
          </div>
        {:else if isSyncing}
          <div class="syncing-accounts">
            <div class="syncing-spinner"></div>
            <p>Syncing accounts...</p>
          </div>
        {:else}
          <div class="no-accounts">
            <p>No accounts synced yet. Run a sync to fetch your accounts.</p>
          </div>
        {/if}

        <div class="integration-details">
          <div class="detail-row">
            <span class="detail-label">Connected:</span>
            <span class="detail-value">{new Date(simplefinIntegration.created_at).toLocaleDateString()}</span>
          </div>
          {#if settings?.app.lastSyncDate}
            <div class="detail-row">
              <span class="detail-label">Last synced:</span>
              <span class="detail-value">{formatLastSync(settings.app.lastSyncDate)}</span>
            </div>
          {/if}
        </div>

        <div class="simplefin-link">
          <button
            class="link-btn"
            onclick={() => onOpenExternalUrl(SIMPLEFIN.url)}
          >
            Manage connections on SimpleFIN
            <Icon name="external-link" size={12} />
          </button>
        </div>
      {:else}
        <div class="integration-status disconnected">
          <span class="status-dot"></span>
          <span>Not connected</span>
        </div>
        <button class="btn primary" onclick={onOpenSetupModal}>
          Connect SimpleFIN
        </button>
      {/if}
    </div>

    <!-- Lunch Flow Integration Card -->
    <div class="integration-card">
      <span class="badge experimental corner">Experimental</span>
      <div class="integration-header">
        <div class="integration-info">
          <div class="integration-title-row">
            <h4 class="integration-name">{LUNCHFLOW.name}</h4>
            <button
              class="link-btn learn-more"
              onclick={() => onOpenExternalUrl(LUNCHFLOW.url)}
            >
              Learn more
              <Icon name="external-link" size={10} />
            </button>
          </div>
          <p class="integration-desc">{LUNCHFLOW.description}</p>
        </div>
        {#if isLunchflowConnected}
          <button class="btn secondary small" onclick={() => openDisconnectConfirm("lunchflow")}>
            Disconnect
          </button>
        {/if}
      </div>

      {#if isLunchflowConnected && lunchflowIntegration}
        <div class="integration-status connected">
          <span class="status-dot"></span>
          <span>Connected</span>
        </div>

        {#if lunchflowAccounts.length > 0}
          <div class="linked-accounts">
            <div class="accounts-header">
              <span class="accounts-title">Linked Accounts ({lunchflowAccounts.length})</span>
            </div>
            <div class="sync-settings-help">
              <Icon name="info" size={14} />
              <span class="help-text">Choose what to sync for each account. Select "Balances only" for accounts where you don't need individual transactions.</span>
            </div>
            {#each [...lunchflowAccountsByInstitution] as [institution, accounts]}
              <div class="institution-group">
                <div class="institution-header">
                  <span class="institution-name">{institution}</span>
                </div>
                <div class="institution-accounts">
                  {#each accounts as account}
                    <div class="account-item">
                      <div class="account-info">
                        <span class="account-name">{account.name}</span>
                        {#if account.account_type}
                          <span class="account-type">{account.account_type}</span>
                        {/if}
                        {#if account.currency && account.currency !== 'USD'}
                          <span class="account-currency">{account.currency}</span>
                        {/if}
                      </div>
                      <div class="segmented-toggle">
                        <button
                          class="toggle-option"
                          class:active={!account.balances_only}
                          onclick={() => { if (account.balances_only) onToggleLunchflowBalancesOnly(account); }}
                        >
                          Balances + Transactions
                        </button>
                        <button
                          class="toggle-option"
                          class:active={account.balances_only}
                          onclick={() => { if (!account.balances_only) onToggleLunchflowBalancesOnly(account); }}
                        >
                          Balances only
                        </button>
                      </div>
                    </div>
                  {/each}
                </div>
              </div>
            {/each}
          </div>
        {:else if isSyncing}
          <div class="syncing-accounts">
            <div class="syncing-spinner"></div>
            <p>Syncing accounts...</p>
          </div>
        {:else}
          <div class="no-accounts">
            <p>No accounts synced yet. Run a sync to fetch your accounts.</p>
          </div>
        {/if}

        <div class="integration-details">
          <div class="detail-row">
            <span class="detail-label">Connected:</span>
            <span class="detail-value">{new Date(lunchflowIntegration.created_at).toLocaleDateString()}</span>
          </div>
          {#if settings?.app.lastSyncDate}
            <div class="detail-row">
              <span class="detail-label">Last synced:</span>
              <span class="detail-value">{formatLastSync(settings.app.lastSyncDate)}</span>
            </div>
          {/if}
        </div>

        <div class="simplefin-link">
          <button
            class="link-btn"
            onclick={() => onOpenExternalUrl(LUNCHFLOW.url)}
          >
            Manage connections on {LUNCHFLOW.name}
            <Icon name="external-link" size={12} />
          </button>
        </div>
      {:else}
        <div class="integration-status disconnected">
          <span class="status-dot"></span>
          <span>Not connected</span>
        </div>
        <button class="btn primary" onclick={onOpenLunchflowSetupModal}>
          Connect {LUNCHFLOW.name}
        </button>
      {/if}
    </div>
  {/if}
</section>

<!-- Disconnect Confirmation Sub-Modal -->
{#if showDisconnectConfirm}
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div class="sub-modal-overlay" onclick={closeDisconnectConfirm} onkeydown={(e) => e.key === 'Escape' && closeDisconnectConfirm()} role="dialog" aria-modal="true" tabindex="-1">
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div class="sub-modal confirm-modal" role="document" onclick={(e) => e.stopPropagation()} onkeydown={(e) => e.stopPropagation()}>
      <div class="sub-modal-header">
        <span class="sub-modal-title">Disconnect Integration?</span>
        <button class="close-btn" onclick={closeDisconnectConfirm}>
          <Icon name="x" size={16} />
        </button>
      </div>
      <div class="sub-modal-body">
        <p>Are you sure you want to disconnect <strong>{disconnectingIntegration}</strong>?</p>
        <p class="confirm-note">Your existing accounts and transactions will remain, but new data won't sync until you reconnect.</p>
      </div>
      <div class="sub-modal-actions">
        <button class="btn secondary" onclick={closeDisconnectConfirm}>Cancel</button>
        <button class="btn danger" onclick={handleDisconnect}>Disconnect</button>
      </div>
    </div>
  </div>
{/if}

<style>
  /* Integration cards */
  .integration-card {
    position: relative;
    background: var(--bg-secondary);
    border: 1px solid var(--border-primary);
    border-radius: 8px;
    padding: var(--spacing-md);
    margin-bottom: var(--spacing-md);
  }

  .integration-card:last-child {
    margin-bottom: 0;
  }

  .integration-header {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    margin-bottom: var(--spacing-sm);
  }

  .integration-info {
    flex: 1;
  }

  .integration-title-row {
    display: flex;
    align-items: center;
    gap: var(--spacing-sm);
    margin-bottom: 4px;
  }

  .integration-name {
    font-size: 14px;
    font-weight: 600;
    color: var(--text-primary);
    margin: 0;
  }

  .learn-more {
    font-size: 11px;
    padding: 0;
  }

  .badge {
    font-size: 9px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.3px;
    padding: 2px 6px;
    border-radius: 3px;
  }

  .badge.experimental {
    background: rgba(245, 158, 11, 0.15);
    color: var(--accent-warning, #f59e0b);
    border: 1px solid rgba(245, 158, 11, 0.3);
  }

  .badge.corner {
    position: absolute;
    top: var(--spacing-sm);
    right: var(--spacing-sm);
  }

  .integration-desc {
    font-size: 12px;
    color: var(--text-muted);
    margin: 0;
    line-height: 1.4;
  }

  .integration-status {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    margin-bottom: var(--spacing-md);
  }

  .integration-status .status-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
  }

  .integration-status.connected .status-dot {
    background: var(--accent-success, #22c55e);
  }

  .integration-status.connected {
    color: var(--accent-success, #22c55e);
  }

  .integration-status.disconnected .status-dot {
    background: var(--text-muted);
  }

  .integration-status.disconnected {
    color: var(--text-muted);
  }

  .integration-status.warning .status-dot {
    background: var(--accent-warning, #f59e0b);
  }

  .integration-status.warning {
    color: var(--accent-warning, #f59e0b);
  }

  .integration-details {
    background: var(--bg-tertiary);
    border-radius: 6px;
    padding: var(--spacing-sm) var(--spacing-md);
    font-size: 11px;
  }

  .detail-row {
    display: flex;
    gap: var(--spacing-sm);
    margin-bottom: 4px;
  }

  .detail-row:last-child {
    margin-bottom: 0;
  }

  .detail-label {
    color: var(--text-muted);
  }

  .detail-value {
    color: var(--text-primary);
  }

  /* Linked accounts */
  .linked-accounts {
    background: var(--bg-tertiary);
    border-radius: 6px;
    padding: var(--spacing-sm) var(--spacing-md);
    margin-bottom: var(--spacing-md);
  }

  .accounts-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: var(--spacing-sm);
  }

  .accounts-title {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-secondary);
  }

  .sync-settings-help {
    display: flex;
    align-items: flex-start;
    gap: var(--spacing-sm);
    padding: var(--spacing-xs) var(--spacing-sm);
    background: var(--bg-secondary);
    border-radius: 4px;
    margin-bottom: var(--spacing-sm);
  }

  .help-text {
    font-size: 11px;
    color: var(--text-muted);
    line-height: 1.4;
  }

  .institution-group {
    margin-bottom: var(--spacing-sm);
  }

  .institution-header {
    display: flex;
    align-items: center;
    gap: var(--spacing-sm);
    margin-bottom: 4px;
  }

  .institution-name {
    font-size: 11px;
    font-weight: 600;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.3px;
  }

  .institution-status {
    font-size: 11px;
    font-weight: 600;
  }

  .institution-status.checking {
    color: var(--text-muted);
  }

  :global(.status-ok) {
    color: var(--accent-success, #22c55e);
  }

  .institution-status.warning {
    color: #efb444;
  }

  .institution-accounts {
    padding-left: var(--spacing-sm);
    border-left: 2px solid var(--border-primary);
  }

  .institution-group.checked-ok .institution-accounts {
    border-left-color: var(--accent-success, #22c55e);
  }

  .institution-group.has-warning .institution-accounts {
    border-left-color: #efb444;
  }

  .account-item {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 4px var(--spacing-sm);
    font-size: 12px;
    gap: var(--spacing-sm);
  }

  .account-info {
    display: flex;
    align-items: center;
    gap: var(--spacing-sm);
    flex: 1;
    min-width: 0;
  }

  .account-name {
    color: var(--text-primary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .account-type {
    font-size: 10px;
    color: var(--text-muted);
    background: var(--bg-secondary);
    padding: 2px 5px;
    border-radius: 3px;
    flex-shrink: 0;
  }

  .account-currency {
    font-size: 10px;
    color: var(--accent-primary);
    background: rgba(59, 130, 246, 0.1);
    padding: 2px 5px;
    border-radius: 3px;
    flex-shrink: 0;
  }

  .segmented-toggle {
    display: flex;
    border: 1px solid var(--border-primary);
    border-radius: 4px;
    overflow: hidden;
    flex-shrink: 0;
  }

  .segmented-toggle .toggle-option {
    background: var(--bg-secondary);
    border: none;
    padding: 3px 8px;
    font-size: 10px;
    color: var(--text-muted);
    cursor: pointer;
    transition: all 0.15s;
    white-space: nowrap;
  }

  .segmented-toggle .toggle-option:not(:last-child) {
    border-right: 1px solid var(--border-primary);
  }

  .segmented-toggle .toggle-option:hover:not(.active) {
    background: var(--bg-tertiary);
  }

  .segmented-toggle .toggle-option.active {
    background: var(--accent-primary);
    color: white;
  }

  .connection-warnings {
    background: rgba(239, 180, 68, 0.1);
    border: 1px solid rgba(239, 180, 68, 0.3);
    border-radius: 4px;
    padding: var(--spacing-sm);
    margin-top: var(--spacing-sm);
  }

  .warning-item {
    display: flex;
    align-items: flex-start;
    gap: var(--spacing-sm);
    margin-bottom: var(--spacing-xs);
  }

  .warning-icon {
    color: #efb444;
    font-weight: bold;
    flex-shrink: 0;
  }

  .warning-text {
    font-size: 12px;
    color: var(--text-primary);
    line-height: 1.4;
  }

  .no-accounts {
    text-align: center;
    padding: var(--spacing-sm);
    color: var(--text-muted);
    font-size: 12px;
  }

  .no-accounts p {
    margin: 0;
  }

  .syncing-accounts {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: var(--spacing-sm);
    padding: var(--spacing-md);
    color: var(--text-secondary);
    font-size: 12px;
  }

  .syncing-accounts p {
    margin: 0;
  }

  .syncing-spinner {
    width: 16px;
    height: 16px;
    border: 2px solid var(--border-primary);
    border-top-color: var(--accent-primary);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  .simplefin-link {
    margin-top: var(--spacing-md);
    padding-top: var(--spacing-sm);
    border-top: 1px solid var(--border-primary);
  }

  /* Confirm modal */
  .confirm-modal .sub-modal-body p {
    margin: 0 0 var(--spacing-xs) 0;
    font-size: 13px;
    color: var(--text-primary);
  }

  .confirm-note {
    font-size: 12px !important;
    color: var(--text-muted) !important;
  }
</style>
