<script lang="ts">
  /**
   * UnlockModal - Password prompt for encrypted database
   *
   * Shown on app startup when database is encrypted but no key is available
   * (either in memory or OS keychain).
   */

  import Modal from "../shared/Modal.svelte";
  import { unlockDatabase, toast } from "../sdk";

  interface Props {
    open: boolean;
    onunlock: () => void;
  }

  let { open = false, onunlock }: Props = $props();

  let password = $state("");
  let isUnlocking = $state(false);
  let error = $state("");
  let passwordInput = $state<HTMLInputElement | null>(null);

  // Focus password input when modal opens
  $effect(() => {
    if (open && passwordInput) {
      setTimeout(() => passwordInput?.focus(), 100);
    }
  });

  // Reset state when modal opens
  $effect(() => {
    if (open) {
      password = "";
      error = "";
      isUnlocking = false;
    }
  });

  async function handleUnlock() {
    if (!password.trim()) {
      error = "Password is required";
      return;
    }

    isUnlocking = true;
    error = "";

    try {
      await unlockDatabase(password);
      toast.success("Database unlocked");
      onunlock();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
      if (error.toLowerCase().includes("invalid password")) {
        error = "Invalid password. Please try again.";
      }
    } finally {
      isUnlocking = false;
    }
  }

  function handleKeyDown(e: KeyboardEvent) {
    if (e.key === "Enter" && !isUnlocking) {
      handleUnlock();
    }
  }

  // Prevent closing via overlay click or escape - must enter password
  function handleClose() {
    // Don't allow closing - user must unlock
  }
</script>

<Modal {open} title="Unlock Database" onclose={handleClose} width="380px">
  <div class="unlock-content">
    <div class="lock-icon">
      <svg
        xmlns="http://www.w3.org/2000/svg"
        width="48"
        height="48"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="1.5"
        stroke-linecap="round"
        stroke-linejoin="round"
      >
        <rect x="3" y="11" width="18" height="11" rx="2" ry="2" />
        <path d="M7 11V7a5 5 0 0 1 10 0v4" />
      </svg>
    </div>

    <p class="description">
      Your database is encrypted. Enter your password to unlock.
    </p>

    {#if error}
      <div class="error-message">{error}</div>
    {/if}

    <div class="form-group">
      <label for="unlock-password">Password</label>
      <input
        id="unlock-password"
        type="password"
        bind:value={password}
        bind:this={passwordInput}
        onkeydown={handleKeyDown}
        placeholder="Enter password"
        disabled={isUnlocking}
        autocomplete="current-password"
      />
    </div>

    <p class="session-hint">
      Your key will be stored in memory for this session only.
    </p>
  </div>

  {#snippet actions()}
    <button
      class="btn primary unlock-btn"
      onclick={handleUnlock}
      disabled={isUnlocking || !password.trim()}
    >
      {#if isUnlocking}
        Unlocking...
      {:else}
        Unlock
      {/if}
    </button>
  {/snippet}
</Modal>

<style>
  .unlock-content {
    padding: var(--spacing-lg);
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
  }

  .lock-icon {
    color: var(--accent-primary);
    margin-bottom: var(--spacing-md);
  }

  .description {
    color: var(--text-secondary);
    font-size: 13px;
    margin: 0 0 var(--spacing-lg) 0;
  }

  .error-message {
    background: rgba(239, 68, 68, 0.1);
    border: 1px solid var(--accent-danger, #ef4444);
    color: var(--accent-danger, #ef4444);
    padding: var(--spacing-sm) var(--spacing-md);
    border-radius: 4px;
    font-size: 12px;
    margin-bottom: var(--spacing-md);
    width: 100%;
  }

  .form-group {
    width: 100%;
    text-align: left;
    margin-bottom: var(--spacing-md);
  }

  .form-group label {
    display: block;
    font-size: 12px;
    font-weight: 600;
    color: var(--text-secondary);
    margin-bottom: var(--spacing-xs);
  }

  .form-group input {
    width: 100%;
    padding: 10px 12px;
    background: var(--bg-primary);
    border: 1px solid var(--border-primary);
    border-radius: 4px;
    color: var(--text-primary);
    font-size: 14px;
  }

  .form-group input:focus {
    outline: none;
    border-color: var(--accent-primary);
  }

  .form-group input:disabled {
    opacity: 0.5;
  }

  .session-hint {
    color: var(--text-muted);
    font-size: 11px;
    margin: var(--spacing-sm) 0 0 0;
    text-align: center;
    width: 100%;
  }

  .unlock-btn {
    width: 100%;
  }

  .unlock-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
</style>
