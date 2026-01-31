<script lang="ts">
  /**
   * Shared form component for creating new accounts
   * Used by both AccountsView and ImportModal
   */

  export type BalanceClassification = "asset" | "liability";

  export interface AddAccountFormData {
    name: string;
    nickname: string;
    institution_name: string;
    account_type: string;
    classification: BalanceClassification;
    initial_balance: string;
  }

  interface Props {
    onsubmit: (data: AddAccountFormData) => void;
    oncancel: () => void;
    isSaving?: boolean;
    showInitialBalance?: boolean;
    showClassification?: boolean;
    submitLabel?: string;
    initialValues?: Partial<AddAccountFormData>;
  }

  let {
    onsubmit,
    oncancel,
    isSaving = false,
    showInitialBalance = true,
    showClassification = true,
    submitLabel = "Add Account",
    initialValues,
  }: Props = $props();

  let form = $state<AddAccountFormData>({
    name: initialValues?.name ?? "",
    nickname: initialValues?.nickname ?? "",
    institution_name: initialValues?.institution_name ?? "",
    account_type: initialValues?.account_type ?? "",
    classification: initialValues?.classification ?? "asset",
    initial_balance: initialValues?.initial_balance ?? "",
  });

  // Track if user has manually changed classification
  let userChangedClassification = $state(!!initialValues?.classification);

  // Auto-switch classification based on account type (credit/loan = liability)
  $effect(() => {
    if (userChangedClassification) return;
    const type = form.account_type.toLowerCase().trim();
    if (type === "credit" || type === "loan") {
      form.classification = "liability";
    } else if (type) {
      form.classification = "asset";
    }
  });

  function handleClassificationChange(value: BalanceClassification) {
    userChangedClassification = true;
    form.classification = value;
  }

  function handleSubmit() {
    if (!form.name.trim()) return;
    onsubmit(form);
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      handleSubmit();
    } else if (e.key === "Escape") {
      oncancel();
    }
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="add-account-form" onkeydown={handleKeydown}>
  <label class="form-field">
    Account Name *
    <input
      type="text"
      bind:value={form.name}
      placeholder="e.g., Home Equity, Cash"
    />
  </label>

  <label class="form-field">
    Nickname (optional)
    <input
      type="text"
      bind:value={form.nickname}
      placeholder="Display name"
    />
  </label>

  <label class="form-field">
    Institution (optional)
    <input
      type="text"
      bind:value={form.institution_name}
      placeholder="e.g., Zillow, Manual"
    />
  </label>

  <label class="form-field">
    Type (optional)
    <input
      type="text"
      bind:value={form.account_type}
      placeholder="depository, credit, investment, loan, other"
    />
    <span class="field-hint">depository, credit, investment, loan, other</span>
  </label>

  {#if showClassification}
    <div class="form-field">
      <span class="field-label">Balance Classification</span>
      <div class="radio-group">
        <label class="radio-option">
          <input
            type="radio"
            name="add-account-classification"
            value="asset"
            checked={form.classification === "asset"}
            onchange={() => handleClassificationChange("asset")}
          />
          Asset
        </label>
        <label class="radio-option">
          <input
            type="radio"
            name="add-account-classification"
            value="liability"
            checked={form.classification === "liability"}
            onchange={() => handleClassificationChange("liability")}
          />
          Liability
        </label>
      </div>
    </div>
  {/if}

  {#if showInitialBalance}
    <label class="form-field">
      Initial Balance (optional)
      <input
        type="text"
        bind:value={form.initial_balance}
        placeholder="0.00"
      />
      <span class="field-hint">
        {#if form.classification === "liability"}
          Amount owed (will be stored as negative)
        {:else}
          Current balance as of today
        {/if}
      </span>
    </label>
  {/if}

  <div class="form-actions">
    <button class="btn-secondary" onclick={oncancel} type="button">Cancel</button>
    <button
      class="btn-primary"
      onclick={handleSubmit}
      disabled={isSaving || !form.name.trim()}
      type="button"
    >
      {isSaving ? "Creating..." : submitLabel}
    </button>
  </div>
</div>

<style>
  .add-account-form {
    display: flex;
    flex-direction: column;
    gap: var(--spacing-md);
  }

  .form-field {
    display: flex;
    flex-direction: column;
    gap: 6px;
    font-size: 12px;
    font-weight: 500;
    color: var(--text-muted);
  }

  .form-field input[type="text"] {
    padding: 8px 12px;
    background: var(--bg-primary);
    border: 1px solid var(--border-primary);
    border-radius: 4px;
    color: var(--text-primary);
    font-size: 13px;
    font-weight: 400;
  }

  .form-field input[type="text"]:focus {
    outline: none;
    border-color: var(--accent-primary);
  }

  .field-label {
    font-size: 12px;
    font-weight: 500;
    color: var(--text-muted);
  }

  .field-hint {
    font-size: 11px;
    font-weight: 400;
    color: var(--text-muted);
    opacity: 0.8;
  }

  .radio-group {
    display: flex;
    gap: var(--spacing-lg);
    margin-top: 4px;
  }

  .radio-option {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 13px;
    font-weight: 400;
    color: var(--text-primary);
    cursor: pointer;
  }

  .radio-option input[type="radio"] {
    margin: 0;
    cursor: pointer;
  }

  .form-actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--spacing-sm);
    margin-top: var(--spacing-sm);
    padding-top: var(--spacing-sm);
    border-top: 1px solid var(--border-primary);
  }

  .btn-secondary {
    padding: 8px 16px;
    background: var(--bg-secondary);
    border: 1px solid var(--border-primary);
    border-radius: 6px;
    color: var(--text-primary);
    font-size: 13px;
    font-weight: 500;
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .btn-secondary:hover {
    background: var(--bg-tertiary);
    border-color: var(--text-muted);
  }

  .btn-primary {
    padding: 8px 16px;
    background: var(--accent-primary);
    border: 1px solid var(--accent-primary);
    border-radius: 6px;
    color: white;
    font-size: 13px;
    font-weight: 500;
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .btn-primary:hover:not(:disabled) {
    opacity: 0.9;
  }

  .btn-primary:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }
</style>
