<script lang="ts">
  import { Icon } from "../../../shared";
  import {
    listBackups,
    createBackup,
    restoreBackup,
    deleteBackup,
    clearBackups,
    compactDatabase,
    getEncryptionStatus,
    enableEncryption,
    disableEncryption,
    formatBytes,
    toast,
    type EncryptionStatus,
    type BackupMetadata,
  } from "../../../sdk";
  import "../settings-shared.css";

  interface Props {
    isDemoMode: boolean;
  }

  let { isDemoMode }: Props = $props();

  // Encryption state
  let encryptionStatus = $state<EncryptionStatus | null>(null);
  let isLoadingEncryption = $state(true);
  let showEncryptModal = $state(false);
  let showDecryptModal = $state(false);
  let encryptPassword = $state("");
  let encryptPasswordConfirm = $state("");
  let decryptPassword = $state("");
  let isEncrypting = $state(false);
  let isDecrypting = $state(false);
  let encryptError = $state("");

  // Backup state
  let backups = $state<BackupMetadata[]>([]);
  let isLoadingBackups = $state(true);
  let isCreatingBackup = $state(false);
  let isRestoringBackup = $state<string | null>(null);
  let isDeletingBackup = $state<string | null>(null);
  let isClearingBackups = $state(false);
  let isCompacting = $state(false);

  // Load data on mount
  $effect(() => {
    loadEncryptionStatus();
    loadBackups();
  });

  async function loadEncryptionStatus() {
    isLoadingEncryption = true;
    try {
      encryptionStatus = await getEncryptionStatus();
    } catch (e) {
      console.error("Failed to load encryption status:", e);
    } finally {
      isLoadingEncryption = false;
    }
  }

  async function loadBackups() {
    isLoadingBackups = true;
    try {
      backups = await listBackups();
    } catch (e) {
      console.error("Failed to load backups:", e);
    } finally {
      isLoadingBackups = false;
    }
  }

  // Encryption handlers
  function openEncryptModal() {
    encryptPassword = "";
    encryptPasswordConfirm = "";
    encryptError = "";
    showEncryptModal = true;
  }

  function closeEncryptModal() {
    showEncryptModal = false;
    encryptPassword = "";
    encryptPasswordConfirm = "";
    encryptError = "";
  }

  async function handleEnableEncryption() {
    if (encryptPassword.length < 8) {
      encryptError = "Password must be at least 8 characters";
      return;
    }
    if (encryptPassword !== encryptPasswordConfirm) {
      encryptError = "Passwords do not match";
      return;
    }

    isEncrypting = true;
    encryptError = "";
    try {
      await enableEncryption(encryptPassword);
      toast.success("Encryption enabled", "Your database is now encrypted");
      closeEncryptModal();
      await loadEncryptionStatus();
    } catch (e) {
      encryptError = e instanceof Error ? e.message : String(e);
    } finally {
      isEncrypting = false;
    }
  }

  function openDecryptModal() {
    decryptPassword = "";
    encryptError = "";
    showDecryptModal = true;
  }

  function closeDecryptModal() {
    showDecryptModal = false;
    decryptPassword = "";
    encryptError = "";
  }

  async function handleDisableEncryption() {
    isDecrypting = true;
    encryptError = "";
    try {
      await disableEncryption(decryptPassword);
      toast.success("Encryption disabled", "Your database is no longer encrypted");
      closeDecryptModal();
      await loadEncryptionStatus();
    } catch (e) {
      encryptError = e instanceof Error ? e.message : String(e);
    } finally {
      isDecrypting = false;
    }
  }

  // Backup handlers
  async function handleCreateBackup() {
    isCreatingBackup = true;
    try {
      const result = await createBackup(10);
      toast.success(`Backup created: ${result.name}`);
      await loadBackups();
    } catch (e) {
      toast.error(`Failed to create backup: ${e}`);
    } finally {
      isCreatingBackup = false;
    }
  }

  async function handleRestoreBackup(backupName: string) {
    if (!confirm(`Restore from ${backupName}?\n\nThis will replace your current data. A pre-restore backup will be created automatically.`)) {
      return;
    }

    isRestoringBackup = backupName;
    try {
      await restoreBackup(backupName);
      toast.success("Backup restored! Please restart the app.");
    } catch (e) {
      toast.error(`Failed to restore backup: ${e}`);
    } finally {
      isRestoringBackup = null;
    }
  }

  async function handleDeleteBackup(backupName: string) {
    if (!confirm(`Delete backup ${backupName}?`)) {
      return;
    }

    isDeletingBackup = backupName;
    try {
      await deleteBackup(backupName);
      toast.success("Backup deleted");
      await loadBackups();
    } catch (e) {
      toast.error(`Failed to delete backup: ${e}`);
    } finally {
      isDeletingBackup = null;
    }
  }

  async function handleClearBackups() {
    if (!confirm(`Delete all ${backups.length} backups?\n\nThis cannot be undone.`)) {
      return;
    }

    isClearingBackups = true;
    try {
      const result = await clearBackups();
      toast.success(`Cleared ${result.deleted_count} backups`);
      await loadBackups();
    } catch (e) {
      toast.error(`Failed to clear backups: ${e}`);
    } finally {
      isClearingBackups = false;
    }
  }

  async function handleCompact() {
    isCompacting = true;
    try {
      const result = await compactDatabase();
      const saved = result.original_size - result.compacted_size;
      if (saved > 0) {
        toast.success(`Database compacted. Saved ${formatBytes(saved)} (${formatBytes(result.original_size)} → ${formatBytes(result.compacted_size)})`);
      } else {
        toast.info(`Database already optimized (${formatBytes(result.compacted_size)})`);
      }
    } catch (e) {
      toast.error(`Failed to compact database: ${e}`);
    } finally {
      isCompacting = false;
    }
  }
</script>

<section class="section">
  <h3 class="section-title">Storage</h3>

  <div class="setting-group">
    <h4 class="group-title">Database Encryption</h4>

    {#if isDemoMode}
      <div class="demo-warning">
        <Icon name="alert-triangle" size={16} />
        <span>Encryption is not available in demo mode</span>
      </div>
    {:else if isLoadingEncryption}
      <div class="loading">Loading encryption status...</div>
    {:else}
      <div class="encryption-status">
        <div class="status-row">
          <span class="status-label">Status:</span>
          <span class="status-value" class:encrypted={encryptionStatus?.encrypted}>
            {#if encryptionStatus?.encrypted}
              <Icon name="lock" size={14} />
              Encrypted
            {:else}
              <Icon name="unlock" size={14} />
              Not encrypted
            {/if}
          </span>
        </div>
        {#if encryptionStatus?.encrypted}
          <div class="status-row">
            <span class="status-label">Algorithm:</span>
            <span class="status-value">{encryptionStatus.algorithm || "Unknown"}</span>
          </div>
        {/if}
      </div>

      <p class="setting-description">
        Encrypt your database to protect your financial data at rest.
        Uses AES-256-GCM encryption with Argon2id key derivation.
      </p>

      <div class="encryption-actions">
        {#if encryptionStatus?.encrypted}
          <button class="btn danger" onclick={openDecryptModal}>
            <Icon name="unlock" size={14} />
            Disable Encryption
          </button>
        {:else}
          <button class="btn primary" onclick={openEncryptModal}>
            <Icon name="lock" size={14} />
            Enable Encryption
          </button>
        {/if}
      </div>

      {#if encryptionStatus?.encrypted}
        <p class="encryption-hint">
          You'll need to enter your password each time you open the app.
        </p>
      {:else}
        <p class="encryption-hint">
          After enabling encryption, you'll need to enter your password
          each time you open the app.
        </p>
      {/if}
    {/if}
  </div>

  <div class="setting-group">
    <h4 class="group-title">Backup & Maintenance</h4>
    <p class="group-desc">Backups are stored locally in ~/.treeline/backups (not synced to cloud).</p>

    <div class="backup-actions">
      <button
        class="btn primary"
        onclick={handleCreateBackup}
        disabled={isCreatingBackup}
      >
        {#if isCreatingBackup}
          <Icon name="refresh" size={14} class="spinning" />
          Creating...
        {:else}
          <Icon name="download" size={14} />
          Create Backup
        {/if}
      </button>

      <button
        class="btn secondary"
        onclick={handleCompact}
        disabled={isCompacting}
      >
        {#if isCompacting}
          <Icon name="refresh" size={14} class="spinning" />
          Compacting...
        {:else}
          <Icon name="zap" size={14} />
          Compact Database
        {/if}
      </button>
    </div>

    {#if isLoadingBackups}
      <div class="loading-small">Loading backups...</div>
    {:else if backups.length === 0}
      <p class="empty-hint">No backups yet. Create one to protect your data.</p>
    {:else}
      <div class="backup-list">
        <h5 class="backup-list-title">Available Backups ({backups.length})</h5>
        {#each backups as backup}
          {@const date = new Date(backup.created_at)}
          <div class="backup-item">
            <div class="backup-info">
              <span class="backup-date">{date.toLocaleDateString()} {date.toLocaleTimeString()}</span>
              <span class="backup-size">{formatBytes(backup.size_bytes)}</span>
            </div>
            <div class="backup-actions-row">
              <button
                class="btn-backup-action"
                onclick={() => handleRestoreBackup(backup.name)}
                disabled={isRestoringBackup === backup.name}
                title="Restore from this backup"
              >
                {#if isRestoringBackup === backup.name}
                  <Icon name="refresh" size={14} class="spinning" />
                {:else}
                  <Icon name="repeat" size={14} />
                {/if}
              </button>
              <button
                class="btn-backup-action danger"
                onclick={() => handleDeleteBackup(backup.name)}
                disabled={isDeletingBackup === backup.name}
                title="Delete this backup"
              >
                {#if isDeletingBackup === backup.name}
                  <Icon name="refresh" size={14} class="spinning" />
                {:else}
                  <Icon name="trash" size={14} />
                {/if}
              </button>
            </div>
          </div>
        {/each}
        <button
          class="btn secondary clear-all-btn"
          onclick={handleClearBackups}
          disabled={isClearingBackups}
        >
          {#if isClearingBackups}
            <Icon name="refresh" size={14} class="spinning" />
            Clearing...
          {:else}
            <Icon name="trash" size={14} />
            Clear All Backups
          {/if}
        </button>
      </div>
    {/if}
  </div>
</section>

<!-- Enable Encryption Sub-Modal -->
{#if showEncryptModal}
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div class="sub-modal-overlay" onclick={closeEncryptModal} onkeydown={(e) => e.key === 'Escape' && closeEncryptModal()} role="dialog" aria-modal="true" tabindex="-1">
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <div class="sub-modal" role="document" onclick={(e) => e.stopPropagation()}>
      <div class="sub-modal-header">
        <span class="sub-modal-title">Enable Encryption</span>
        <button class="close-btn" onclick={closeEncryptModal}>
          <Icon name="x" size={16} />
        </button>
      </div>
      <div class="sub-modal-body">
        <p class="encrypt-warning">
          Choose a strong password. If you forget it, your data cannot be recovered.
        </p>

        {#if encryptError}
          <div class="encrypt-error">{encryptError}</div>
        {/if}

        <div class="form-group">
          <label for="encrypt-password">Password</label>
          <input
            id="encrypt-password"
            type="password"
            bind:value={encryptPassword}
            placeholder="Enter password (min 8 characters)"
            disabled={isEncrypting}
          />
        </div>

        <div class="form-group">
          <label for="encrypt-password-confirm">Confirm Password</label>
          <input
            id="encrypt-password-confirm"
            type="password"
            bind:value={encryptPasswordConfirm}
            placeholder="Confirm password"
            disabled={isEncrypting}
          />
        </div>
      </div>
      <div class="sub-modal-actions">
        <button class="btn secondary" onclick={closeEncryptModal} disabled={isEncrypting}>Cancel</button>
        <button class="btn primary" onclick={handleEnableEncryption} disabled={isEncrypting || encryptPassword.length < 8 || encryptPassword !== encryptPasswordConfirm}>
          {#if isEncrypting}
            Encrypting...
          {:else}
            Enable Encryption
          {/if}
        </button>
      </div>
    </div>
  </div>
{/if}

<!-- Disable Encryption Sub-Modal -->
{#if showDecryptModal}
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div class="sub-modal-overlay" onclick={closeDecryptModal} onkeydown={(e) => e.key === 'Escape' && closeDecryptModal()} role="dialog" aria-modal="true" tabindex="-1">
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <div class="sub-modal" role="document" onclick={(e) => e.stopPropagation()}>
      <div class="sub-modal-header">
        <span class="sub-modal-title">Disable Encryption</span>
        <button class="close-btn" onclick={closeDecryptModal}>
          <Icon name="x" size={16} />
        </button>
      </div>
      <div class="sub-modal-body">
        <p class="encrypt-warning">
          Enter your current password to disable encryption.
          Your data will be stored unencrypted.
        </p>

        {#if encryptError}
          <div class="encrypt-error">{encryptError}</div>
        {/if}

        <div class="form-group">
          <label for="decrypt-password">Current Password</label>
          <input
            id="decrypt-password"
            type="password"
            bind:value={decryptPassword}
            placeholder="Enter current password"
            disabled={isDecrypting}
          />
        </div>
      </div>
      <div class="sub-modal-actions">
        <button class="btn secondary" onclick={closeDecryptModal} disabled={isDecrypting}>Cancel</button>
        <button class="btn danger" onclick={handleDisableEncryption} disabled={isDecrypting || !decryptPassword}>
          {#if isDecrypting}
            Decrypting...
          {:else}
            Disable Encryption
          {/if}
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
  /* Encryption styles */
  .encryption-status {
    background: var(--bg-tertiary);
    border: 1px solid var(--border-primary);
    border-radius: 6px;
    padding: var(--spacing-md);
    margin-bottom: var(--spacing-md);
  }

  .encryption-status .status-row {
    display: flex;
    align-items: center;
    gap: var(--spacing-sm);
    margin-bottom: var(--spacing-xs);
  }

  .encryption-status .status-row:last-child {
    margin-bottom: 0;
  }

  .encryption-status .status-label {
    color: var(--text-muted);
    font-size: 12px;
    min-width: 80px;
  }

  .encryption-status .status-value {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 13px;
    color: var(--text-secondary);
  }

  .encryption-status .status-value.encrypted {
    color: var(--accent-success, #22c55e);
  }

  .setting-description {
    font-size: 12px;
    color: var(--text-muted);
    margin: 0 0 var(--spacing-md) 0;
    line-height: 1.4;
  }

  .encryption-actions {
    display: flex;
    gap: var(--spacing-sm);
    margin-bottom: var(--spacing-md);
  }

  .encryption-actions .btn {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .encryption-hint {
    font-size: 11px;
    color: var(--text-muted);
    margin: 0;
    line-height: 1.5;
  }

  .encrypt-warning {
    color: var(--accent-warning, #f59e0b);
    font-size: 13px;
    margin: 0 0 var(--spacing-md) 0;
    padding: var(--spacing-sm) var(--spacing-md);
    background: rgba(245, 158, 11, 0.1);
    border-radius: 4px;
  }

  .encrypt-error {
    color: var(--accent-danger, #ef4444);
    font-size: 12px;
    padding: var(--spacing-sm) var(--spacing-md);
    background: rgba(239, 68, 68, 0.1);
    border: 1px solid var(--accent-danger, #ef4444);
    border-radius: 4px;
    margin-bottom: var(--spacing-md);
  }

  /* Backup styles */
  .backup-actions {
    display: flex;
    gap: var(--spacing-sm);
    margin-bottom: var(--spacing-md);
  }

  .backup-list {
    display: flex;
    flex-direction: column;
    gap: var(--spacing-sm);
  }

  .backup-list-title {
    font-size: 12px;
    font-weight: 500;
    color: var(--text-secondary);
    margin: 0 0 var(--spacing-xs) 0;
  }

  .backup-item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--spacing-sm) var(--spacing-md);
    background: var(--bg-secondary);
    border: 1px solid var(--border-primary);
    border-radius: 6px;
    gap: var(--spacing-md);
  }

  .backup-info {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
    flex: 1;
  }

  .backup-date {
    font-size: 13px;
    color: var(--text-primary);
  }

  .backup-size {
    font-size: 11px;
    color: var(--text-muted);
  }

  .backup-actions-row {
    display: flex;
    gap: var(--spacing-xs);
  }

  .btn-backup-action {
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

  .btn-backup-action:hover:not(:disabled) {
    background: var(--bg-tertiary);
    border-color: var(--border-primary);
    color: var(--accent-primary);
  }

  .btn-backup-action.danger:hover:not(:disabled) {
    color: var(--accent-danger);
  }

  .btn-backup-action:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }

  .clear-all-btn {
    margin-top: var(--spacing-md);
    width: 100%;
  }
</style>
