<script lang="ts">
  import { Icon, SUPPORTED_CURRENCIES } from "../../../shared";
  import type { Settings, ImportProfile } from "../../../sdk";
  import "../settings-shared.css";

  interface ImportProfileWithMappings {
    name: string;
    profile: ImportProfile;
    accountNames: string[];
  }

  interface Props {
    settings: Settings;
    currentCurrency: string;
    importProfiles: ImportProfileWithMappings[];
    isLoadingProfiles: boolean;
    deletingProfileName: string | null;
    isSyncing: boolean;
    onCurrencyChange: (currency: string) => void;
    onAutoSyncChange: (enabled: boolean) => void;
    onSync: () => void;
    onDeleteProfile: (name: string) => void;
    formatLastSync: (dateStr: string | null) => string;
  }

  let {
    settings,
    currentCurrency,
    importProfiles,
    isLoadingProfiles,
    deletingProfileName,
    isSyncing,
    onCurrencyChange,
    onAutoSyncChange,
    onSync,
    onDeleteProfile,
    formatLastSync,
  }: Props = $props();
</script>

<section class="section">
  <h3 class="section-title">General</h3>

  <div class="setting-group">
    <h4 class="group-title">Currency</h4>
    <p class="group-desc">Choose the currency for displaying amounts throughout the app. All your accounts should be in this currency.</p>

    <div class="currency-select-wrapper">
      <select
        class="currency-select"
        value={currentCurrency}
        onchange={(e) => onCurrencyChange(e.currentTarget.value)}
      >
        {#each Object.entries(SUPPORTED_CURRENCIES) as [code, info]}
          <option value={code}>{info.symbol} {info.name} ({code})</option>
        {/each}
      </select>
    </div>
  </div>

  <div class="setting-group">
    <h4 class="group-title">Sync</h4>

    <label class="checkbox-setting">
      <input
        type="checkbox"
        checked={settings.app.autoSyncOnStartup}
        onchange={(e) => onAutoSyncChange(e.currentTarget.checked)}
      />
      <span>Auto-sync on startup (once per day)</span>
    </label>

    <div class="setting-row">
      <span class="setting-label">Last synced:</span>
      <span class="setting-value">{formatLastSync(settings.app.lastSyncDate)}</span>
    </div>

    <button
      class="btn primary"
      onclick={onSync}
      disabled={isSyncing}
    >
      {#if isSyncing}
        <Icon name="refresh" size={14} class="spinning" />
        Syncing...
      {:else}
        <Icon name="refresh" size={14} />
        Sync Now
      {/if}
    </button>
  </div>

  <div class="setting-group">
    <h4 class="group-title">Import Profiles</h4>
    <p class="group-desc">Saved column mappings for CSV imports. Profiles can be linked to accounts for automatic selection.</p>

    {#if isLoadingProfiles}
      <div class="loading-small">Loading profiles...</div>
    {:else if importProfiles.length === 0}
      <p class="empty-hint">No saved profiles yet. Create one during CSV import.</p>
    {:else}
      <div class="profile-list">
        {#each importProfiles as { name, profile, accountNames }}
          {@const hasDebitCredit = profile.columnMappings.debit || profile.columnMappings.credit}
          {@const options = [
            profile.options?.flipSigns ? "Flip signs" : null,
            profile.options?.debitNegative ? "Negate debits" : null,
          ].filter(Boolean)}
          <div class="profile-item">
            <div class="profile-info">
              <span class="profile-name">{name}</span>
              <span class="profile-details">
                {hasDebitCredit ? "Debit/Credit columns" : "Single amount column"}
                {#if options.length > 0}
                  <span class="profile-options">• {options.join(", ")}</span>
                {/if}
              </span>
              {#if accountNames.length > 0}
                <span class="profile-accounts">
                  Default for: {accountNames.join(", ")}
                </span>
              {/if}
            </div>
            <button
              class="btn-delete-profile"
              onclick={() => onDeleteProfile(name)}
              disabled={deletingProfileName === name}
              title="Delete profile"
            >
              {#if deletingProfileName === name}
                <Icon name="refresh" size={14} class="spinning" />
              {:else}
                <Icon name="trash" size={14} />
              {/if}
            </button>
          </div>
        {/each}
      </div>
    {/if}
  </div>
</section>

<style>
  /* Currency select */
  .currency-select-wrapper {
    max-width: 280px;
  }

  .currency-select {
    width: 100%;
    padding: 8px 28px 8px 10px;
    background: var(--bg-primary);
    border: 1px solid var(--border-primary);
    border-radius: 6px;
    color: var(--text-primary);
    font-size: 13px;
    appearance: none;
    -webkit-appearance: none;
    background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='12' height='12' viewBox='0 0 12 12'%3E%3Cpath fill='%239ca3af' d='M2 4l4 4 4-4'/%3E%3C/svg%3E");
    background-repeat: no-repeat;
    background-position: right 8px center;
    cursor: pointer;
  }

  .currency-select:focus {
    outline: none;
    border-color: var(--accent-primary);
  }

  .currency-select option {
    background: var(--bg-secondary);
    color: var(--text-primary);
    padding: 8px;
  }

  /* Import profiles */
  .profile-list {
    display: flex;
    flex-direction: column;
    gap: var(--spacing-sm);
  }

  .profile-item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--spacing-sm) var(--spacing-md);
    background: var(--bg-secondary);
    border: 1px solid var(--border-primary);
    border-radius: 6px;
    gap: var(--spacing-md);
  }

  .profile-info {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
    flex: 1;
  }

  .profile-name {
    font-size: 13px;
    font-weight: 500;
    color: var(--text-primary);
  }

  .profile-details {
    font-size: 11px;
    color: var(--text-muted);
  }

  .profile-accounts {
    font-size: 11px;
    color: var(--accent-primary);
  }

  .profile-options {
    color: var(--text-secondary);
    margin-left: 4px;
  }

  .btn-delete-profile {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    background: transparent;
    border: 1px solid transparent;
    border-radius: 4px;
    color: var(--text-muted);
    cursor: pointer;
    transition: all 0.15s;
    flex-shrink: 0;
  }

  .btn-delete-profile:hover:not(:disabled) {
    background: var(--bg-tertiary);
    border-color: var(--border-primary);
    color: var(--accent-danger);
  }

  .btn-delete-profile:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }
</style>
