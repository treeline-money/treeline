<script lang="ts">
  /**
   * AccountsView - Simplified account management
   *
   * Core features:
   * - Account list grouped by Assets/Liabilities
   * - Balance history chart for selected account
   * - Add/Edit/Delete accounts
   * - Set balance, Import CSV
   * - Keyboard navigation
   */
  import {
    Modal,
    RowMenu,
    ActionBar,
    AddOrUpdateAccountForm,
    LineAreaChart,
    formatUserCurrency,
    type RowMenuItem,
    type ActionItem,
    type DataPoint,
    type AddAccountFormData,
  } from "../../shared";
  import { executeQuery, showToast, deleteAccount } from "../../sdk";
  import { registry } from "../../sdk/registry";
  import type { AccountWithStats, BalanceClassification } from "./types";
  import { getDefaultClassification } from "./types";
  import SetBalanceModal from "./SetBalanceModal.svelte";
  import ImportModal from "../../core/ImportModal.svelte";
  import RecalculateBalancesModal from "./RecalculateBalancesModal.svelte";

  // ============================================================================
  // State
  // ============================================================================

  // Account data
  let accounts = $state<AccountWithStats[]>([]);
  let isLoading = $state(true);


  // Selection & navigation
  let viewEl = $state<HTMLDivElement | null>(null);
  let selectedAccountId = $state<string | null>(null);
  let cursorIndex = $state(0);
  let openMenuId = $state<string | null>(null);

  // Detail panel data
  let balanceHistory = $state<DataPoint[]>([]);
  let snapshots = $state<{ id: string; date: string; balance: number; source: string }[]>([]);

  // Modals
  let showAddModal = $state(false);
  let showEditModal = $state(false);
  let showSetBalanceModal = $state(false);
  let showCsvImportModal = $state(false);
  let showRecalculateModal = $state(false);
  let showDeleteConfirm = $state(false);
  let isSaving = $state(false);

  // Edit form initial values (computed when edit modal opens)
  let editInitialValues = $state<Partial<AddAccountFormData> | undefined>(undefined);

  // ============================================================================
  // Derived
  // ============================================================================

  let assetAccounts = $derived(
    accounts.filter((a) => a.classification === "asset")
  );

  let liabilityAccounts = $derived(
    accounts.filter((a) => a.classification === "liability")
  );

  let allAccountsList = $derived([...assetAccounts, ...liabilityAccounts]);

  let selectedAccount = $derived(
    accounts.find((a) => a.account_id === selectedAccountId) || null
  );

  // Keyboard actions
  let keyboardActions: ActionItem[] = $derived([
    { keys: ["j", "k"], label: "navigate" },
    { keys: ["a"], label: "add", action: () => { showAddModal = true; } },
    { keys: ["e"], label: "edit", action: () => handleEdit(), disabled: !selectedAccount },
    { keys: ["s"], label: "add balance", action: () => { showSetBalanceModal = true; }, disabled: !selectedAccount },
    { keys: ["r"], label: "recalculate", action: () => { showRecalculateModal = true; }, disabled: !selectedAccount },
    { keys: ["Enter"], label: "view txns", action: () => handleViewTransactions(), disabled: !selectedAccount },
  ]);

  // ============================================================================
  // Data Loading
  // ============================================================================

  async function loadAccounts() {
    isLoading = true;
    try {
      const result = await executeQuery(`
        WITH account_stats AS (
          SELECT
            account_id,
            COUNT(*) as transaction_count,
            MIN(transaction_date) as first_transaction,
            MAX(transaction_date) as last_transaction,
            SUM(amount) as computed_balance
          FROM transactions
          GROUP BY account_id
        ),
        latest_snapshot AS (
          SELECT
            account_id,
            balance,
            snapshot_time as balance_as_of,
            ROW_NUMBER() OVER (PARTITION BY account_id ORDER BY snapshot_time DESC) as rn
          FROM sys_balance_snapshots
        )
        SELECT
          a.account_id,
          a.name,
          a.nickname,
          a.account_type,
          a.currency,
          ls.balance as balance,
          a.institution_name,
          a.created_at,
          a.updated_at,
          COALESCE(stats.transaction_count, 0) as transaction_count,
          stats.first_transaction,
          stats.last_transaction,
          COALESCE(stats.computed_balance, 0) as computed_balance,
          ls.balance_as_of,
          a.is_manual,
          a.classification
        FROM sys_accounts a
        LEFT JOIN account_stats stats ON a.account_id = stats.account_id
        LEFT JOIN latest_snapshot ls ON a.account_id = ls.account_id AND ls.rn = 1
        ORDER BY a.name
      `);

      accounts = result.rows.map((row) => {
        const accountType = row[3] as string | null;
        const isManual = row[14] as boolean | null;
        const classification = (row[15] as BalanceClassification) || getDefaultClassification(accountType);

        return {
          account_id: row[0] as string,
          name: row[1] as string,
          nickname: row[2] as string | null,
          account_type: accountType,
          currency: row[4] as string,
          balance: row[5] as number | null,
          institution_name: row[6] as string | null,
          created_at: row[7] as string,
          updated_at: row[8] as string,
          transaction_count: row[9] as number,
          first_transaction: row[10] as string | null,
          last_transaction: row[11] as string | null,
          computed_balance: row[12] as number,
          balance_as_of: row[13] as string | null,
          classification,
          isManual: isManual ?? false,
        };
      });

      // Auto-select first account if none selected
      if (!selectedAccountId && accounts.length > 0) {
        selectedAccountId = accounts[0].account_id;
      }
    } catch (e) {
      showToast({ type: "error", title: "Failed to load accounts", message: e instanceof Error ? e.message : "" });
    } finally {
      isLoading = false;
    }
  }

  async function loadDetailData(accountId: string) {
    try {
      // Load balance history for chart
      const historyResult = await executeQuery(`
        SELECT snapshot_time, balance
        FROM sys_balance_snapshots
        WHERE account_id = '${accountId}'
        ORDER BY snapshot_time ASC
      `);

      balanceHistory = historyResult.rows.map((row) => {
        const date = new Date(row[0] as string);
        return {
          label: date.toLocaleDateString("en-US", { month: "short", day: "numeric" }),
          value: row[1] as number,
        };
      });

      // Load recent snapshots for table
      const snapshotsResult = await executeQuery(`
        SELECT snapshot_id, snapshot_time, balance, source
        FROM sys_balance_snapshots
        WHERE account_id = '${accountId}'
        ORDER BY snapshot_time DESC
        LIMIT 200
      `);

      snapshots = snapshotsResult.rows.map((row) => ({
        id: row[0] as string,
        date: (row[1] as string).split("T")[0],
        balance: row[2] as number,
        source: (row[3] as string) || "unknown",
      }));
    } catch (e) {
      console.error("Failed to load detail data", e);
      balanceHistory = [];
      snapshots = [];
    }
  }

  // Load detail when selection changes
  $effect(() => {
    if (selectedAccountId) {
      loadDetailData(selectedAccountId);
    } else {
      balanceHistory = [];
      snapshots = [];
    }
  });

  // Initial load
  $effect(() => {
    loadAccounts();

    // Subscribe to data refresh events
    const unsubscribe = registry.on("data:refresh", () => {
      loadAccounts();
    });

    return unsubscribe;
  });

  // ============================================================================
  // Actions
  // ============================================================================

  function handleSelectAccount(accountId: string) {
    selectedAccountId = accountId;
    // Update cursor to match selection
    const idx = allAccountsList.findIndex((a) => a.account_id === accountId);
    if (idx >= 0) cursorIndex = idx;
  }

  async function handleAddAccount(data: AddAccountFormData) {
    isSaving = true;
    try {
      const accountId = crypto.randomUUID();
      const now = new Date().toISOString();

      await executeQuery(
        `INSERT INTO sys_accounts (account_id, name, nickname, institution_name, account_type, classification, currency, created_at, updated_at)
         VALUES ('${accountId}', '${data.name.replace(/'/g, "''")}', '${data.nickname.replace(/'/g, "''")}',
                 '${data.institution_name.replace(/'/g, "''")}', '${data.account_type}', '${data.classification}', 'USD', '${now}', '${now}')`,
        { readonly: false }
      );

      // Add initial balance if provided
      if (data.initial_balance) {
        const balance = parseFloat(data.initial_balance);
        if (!isNaN(balance)) {
          const finalBalance = data.classification === "liability" ? -Math.abs(balance) : balance;
          const snapshotId = crypto.randomUUID();
          await executeQuery(
            `INSERT INTO sys_balance_snapshots (snapshot_id, account_id, balance, snapshot_time, source, created_at, updated_at)
             VALUES ('${snapshotId}', '${accountId}', ${finalBalance}, '${now}', 'manual', '${now}', '${now}')`,
            { readonly: false }
          );
        }
      }

      showAddModal = false;
      showToast({ type: "success", title: "Account created" });
      await loadAccounts();
      selectedAccountId = accountId;
      registry.emit("data:refresh");
    } catch (e) {
      showToast({ type: "error", title: "Failed to create account", message: e instanceof Error ? e.message : "" });
    } finally {
      isSaving = false;
    }
  }

  function handleEdit() {
    if (!selectedAccount) return;
    editInitialValues = {
      name: selectedAccount.name,
      nickname: selectedAccount.nickname || "",
      institution_name: selectedAccount.institution_name || "",
      account_type: selectedAccount.account_type || "",
      classification: selectedAccount.classification,
    };
    showEditModal = true;
  }

  async function handleSaveEdit(data: AddAccountFormData) {
    if (!selectedAccount) return;
    isSaving = true;
    try {
      const now = new Date().toISOString();

      await executeQuery(
        `UPDATE sys_accounts SET
          name = '${data.name.replace(/'/g, "''")}',
          nickname = '${data.nickname.replace(/'/g, "''")}',
          institution_name = '${data.institution_name.replace(/'/g, "''")}',
          account_type = '${data.account_type}',
          classification = '${data.classification}',
          updated_at = '${now}'
         WHERE account_id = '${selectedAccount.account_id}'`,
        { readonly: false }
      );

      showEditModal = false;
      editInitialValues = undefined;
      showToast({ type: "success", title: "Account updated" });
      await loadAccounts();
      registry.emit("data:refresh");
    } catch (e) {
      showToast({ type: "error", title: "Failed to update account", message: e instanceof Error ? e.message : "" });
    } finally {
      isSaving = false;
    }
  }

  async function handleDelete() {
    if (!selectedAccount) return;
    isSaving = true;
    try {
      // Use rust-core cascading delete (handles transactions, snapshots, account)
      await deleteAccount(selectedAccount.account_id);

      showDeleteConfirm = false;
      showToast({ type: "success", title: "Account deleted" });
      selectedAccountId = null;
      await loadAccounts();
      registry.emit("data:refresh");
    } catch (e) {
      showToast({ type: "error", title: "Failed to delete account", message: e instanceof Error ? e.message : "" });
    } finally {
      isSaving = false;
    }
  }

  async function handleSetBalanceSave(_balance: number, _date: string) {
    showSetBalanceModal = false;
    await loadAccounts();
    if (selectedAccountId) {
      await loadDetailData(selectedAccountId);
    }
    registry.emit("data:refresh");
  }

  function handleCsvImportSuccess(_batchId: string) {
    showCsvImportModal = false;
    loadAccounts();
    if (selectedAccountId) {
      loadDetailData(selectedAccountId);
    }
    registry.emit("data:refresh");
  }

  async function handleDeleteSnapshot(snapshotId: string) {
    try {
      await executeQuery(
        `DELETE FROM sys_balance_snapshots WHERE snapshot_id = '${snapshotId}'`,
        { readonly: false }
      );
      if (selectedAccountId) {
        await loadDetailData(selectedAccountId);
      }
      await loadAccounts();
      registry.emit("data:refresh");
    } catch {
      showToast({ type: "error", title: "Failed to delete snapshot" });
    }
  }

  function handleViewTransactions() {
    if (!selectedAccount) return;
    registry.openView("transactions", { accountId: selectedAccount.account_id });
  }

  // ============================================================================
  // Keyboard Navigation
  // ============================================================================

  function handleKeyDown(e: KeyboardEvent) {
    // Only handle if this view is focused (not another tab)
    if (!viewEl?.contains(document.activeElement) && document.activeElement !== document.body) {
      return;
    }

    // Don't handle if in input/modal
    if (
      e.target instanceof HTMLInputElement ||
      e.target instanceof HTMLTextAreaElement ||
      showAddModal || showEditModal || showSetBalanceModal || showCsvImportModal || showRecalculateModal || showDeleteConfirm
    ) {
      return;
    }

    switch (e.key) {
      case "j":
      case "ArrowDown":
        e.preventDefault();
        if (cursorIndex < allAccountsList.length - 1) {
          cursorIndex++;
          selectedAccountId = allAccountsList[cursorIndex].account_id;
        }
        break;
      case "k":
      case "ArrowUp":
        e.preventDefault();
        if (cursorIndex > 0) {
          cursorIndex--;
          selectedAccountId = allAccountsList[cursorIndex].account_id;
        }
        break;
      case "a":
        e.preventDefault();
        showAddModal = true;
        break;
      case "e":
        e.preventDefault();
        if (selectedAccount) handleEdit();
        break;
      case "s":
        e.preventDefault();
        if (selectedAccount) showSetBalanceModal = true;
        break;
      case "r":
        e.preventDefault();
        if (selectedAccount) showRecalculateModal = true;
        break;
      case "i":
        e.preventDefault();
        if (selectedAccount) showCsvImportModal = true;
        break;
      case "d":
        e.preventDefault();
        if (selectedAccount) showDeleteConfirm = true;
        break;
      case "Enter":
        e.preventDefault();
        handleViewTransactions();
        break;
    }
  }

  // Row menu items
  function getRowMenuItems(account: AccountWithStats): RowMenuItem[] {
    return [
      { label: "Edit", action: () => { selectedAccountId = account.account_id; handleEdit(); } },
      { label: "Add Balance", action: () => { selectedAccountId = account.account_id; showSetBalanceModal = true; } },
      { label: "Recalculate Balances", action: () => { selectedAccountId = account.account_id; showRecalculateModal = true; } },
      { label: "Import CSV", action: () => { selectedAccountId = account.account_id; showCsvImportModal = true; } },
      {
        label: "Delete",
        action: () => { selectedAccountId = account.account_id; showDeleteConfirm = true; },
        danger: true,
      },
    ];
  }

  // Format display name
  function getDisplayName(account: AccountWithStats): string {
    return account.nickname || account.name;
  }

  function getSubtitle(account: AccountWithStats): string {
    if (account.nickname && account.name !== account.nickname) {
      return account.name;
    }
    return account.institution_name || "";
  }
</script>

<svelte:window onkeydown={handleKeyDown} />

<div class="accounts-view" bind:this={viewEl}>
  <!-- Header -->
  <header class="view-header">
    <h1>Accounts</h1>
    <button class="btn-add" onclick={() => { showAddModal = true; }}>+ Add Account</button>
  </header>

  {#if isLoading}
    <div class="loading">
      <div class="spinner"></div>
      <span>Loading accounts...</span>
    </div>
  {:else if accounts.length === 0}
    <!-- Empty state -->
    <div class="empty-state">
      <div class="empty-content">
        <h2>No accounts yet</h2>
        <p>Add your first account to start tracking your financial data.</p>
        <div class="empty-actions">
          <button class="btn-primary" onclick={() => { showAddModal = true; }}>+ Add Account</button>
          <button class="btn-secondary" onclick={() => registry.executeCommand("data:import")}>Import CSV</button>
          <button class="btn-secondary" onclick={() => registry.executeCommand("core:settings:integrations")}>Connect Bank</button>
        </div>
      </div>
    </div>
  {:else}
    <div class="main-content">
      <!-- Account List -->
      <div class="account-list">
        {#if assetAccounts.length > 0}
          <div class="section">
            <div class="section-header">ASSETS</div>
            {#each assetAccounts as account, i}
              {@const globalIndex = i}
              <button
                class="row"
                class:selected={selectedAccountId === account.account_id}
                class:cursor={cursorIndex === globalIndex}
                onclick={() => handleSelectAccount(account.account_id)}
              >
                <div class="row-info">
                  <span class="row-name">{getDisplayName(account)}</span>
                  {#if getSubtitle(account)}
                    <span class="row-subtitle">{getSubtitle(account)}</span>
                  {/if}
                </div>
                {#if account.balance !== null}
                  <span class="row-balance">{formatUserCurrency(account.balance)}</span>
                {:else}
                  <span class="row-balance no-balance">—</span>
                {/if}
                <RowMenu
                  items={getRowMenuItems(account)}
                  isOpen={openMenuId === account.account_id}
                  onToggle={() => { openMenuId = openMenuId === account.account_id ? null : account.account_id; }}
                  onClose={() => { openMenuId = null; }}
                />
              </button>
            {/each}
          </div>
        {/if}

        {#if liabilityAccounts.length > 0}
          <div class="section">
            <div class="section-header">LIABILITIES</div>
            {#each liabilityAccounts as account, i}
              {@const globalIndex = assetAccounts.length + i}
              <button
                class="row"
                class:selected={selectedAccountId === account.account_id}
                class:cursor={cursorIndex === globalIndex}
                onclick={() => handleSelectAccount(account.account_id)}
              >
                <div class="row-info">
                  <span class="row-name">{getDisplayName(account)}</span>
                  {#if getSubtitle(account)}
                    <span class="row-subtitle">{getSubtitle(account)}</span>
                  {/if}
                </div>
                {#if account.balance !== null}
                  <span class="row-balance negative">{formatUserCurrency(Math.abs(account.balance))}</span>
                {:else}
                  <span class="row-balance no-balance">—</span>
                {/if}
                <RowMenu
                  items={getRowMenuItems(account)}
                  isOpen={openMenuId === account.account_id}
                  onToggle={() => { openMenuId = openMenuId === account.account_id ? null : account.account_id; }}
                  onClose={() => { openMenuId = null; }}
                />
              </button>
            {/each}
          </div>
        {/if}
      </div>

      <!-- Detail Panel -->
      {#if selectedAccount}
        <div class="detail-panel">
          <div class="detail-header">
            <span class="detail-name">{getDisplayName(selectedAccount)}</span>
            {#if selectedAccount.institution_name}
              <span class="detail-institution">{selectedAccount.institution_name}</span>
            {/if}
          </div>

          {#if selectedAccount.balance !== null}
            <div class="detail-balance">
              {formatUserCurrency(Math.abs(selectedAccount.balance))}
            </div>
          {:else}
            <div class="detail-balance no-balance">
              <span>No balance set</span>
              <button class="btn-link" onclick={() => { showRecalculateModal = true; }}>Set balance →</button>
            </div>
          {/if}

          <!-- Balance History Chart -->
          <div class="chart-section">
            <div class="chart-label">BALANCE HISTORY</div>
            {#if balanceHistory.length >= 2}
              <LineAreaChart
                data={balanceHistory}
                height={140}
                formatValue={(v) => formatUserCurrency(v)}
                invertTrend={selectedAccount.classification === "liability"}
              />
            {:else}
              <div class="chart-empty">
                <p>No balance history yet.</p>
                <p class="hint">Set a balance to start tracking.</p>
              </div>
            {/if}
          </div>

          <!-- Action Buttons -->
          <div class="detail-actions">
            <button class="action-btn" onclick={() => { showCsvImportModal = true; }}>Import CSV</button>
            <button class="action-btn" onclick={handleViewTransactions}>View Transactions</button>
            <button class="action-btn" onclick={handleEdit}>Edit</button>
          </div>

          <!-- Snapshots Section -->
          <div class="snapshots-section">
            <div class="snapshots-header">
              <div class="snapshots-label">SNAPSHOTS</div>
              <div class="snapshots-actions">
                <button class="snapshots-btn" onclick={() => { showSetBalanceModal = true; }}>+ Add Balance</button>
                <button class="snapshots-btn" onclick={() => { showRecalculateModal = true; }}>Recalculate</button>
              </div>
            </div>
            {#if snapshots.length > 0}
              <div class="snapshots-table-wrapper">
                <div class="snapshots-table">
                  {#each snapshots as snap}
                    <div class="snapshot-row">
                      <span class="snapshot-date">{snap.date}</span>
                      <span class="snapshot-balance">{formatUserCurrency(snap.balance)}</span>
                      <span class="snapshot-source">{snap.source}</span>
                      <button class="snapshot-delete" onclick={() => handleDeleteSnapshot(snap.id)} title="Delete snapshot">
                        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                          <polyline points="3 6 5 6 21 6"></polyline>
                          <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"></path>
                        </svg>
                      </button>
                    </div>
                  {/each}
                </div>
              </div>
            {:else}
              <div class="snapshots-empty">
                No balance snapshots yet. Add a balance to start tracking.
              </div>
            {/if}
          </div>

          <!-- Metadata -->
          <div class="detail-meta">
            {selectedAccount.transaction_count} transactions
            {#if selectedAccount.first_transaction}
              · First: {selectedAccount.first_transaction.split("T")[0]}
            {/if}
            {#if selectedAccount.last_transaction}
              · Last: {selectedAccount.last_transaction.split("T")[0]}
            {/if}
          </div>
        </div>
      {/if}
    </div>
  {/if}

  <!-- Action Bar -->
  <ActionBar actions={keyboardActions} />

  <!-- Add Account Modal -->
  <Modal open={showAddModal} title="Add Account" onclose={() => { showAddModal = false; }}>
    <div class="modal-body">
      <AddOrUpdateAccountForm
        onsubmit={handleAddAccount}
        oncancel={() => { showAddModal = false; }}
        isSaving={isSaving}
      />
    </div>
  </Modal>

  <!-- Edit Account Modal -->
  <Modal open={showEditModal} title="Edit Account" onclose={() => { showEditModal = false; editInitialValues = undefined; }}>
    <div class="modal-body">
      {#key editInitialValues}
        <AddOrUpdateAccountForm
          onsubmit={handleSaveEdit}
          oncancel={() => { showEditModal = false; editInitialValues = undefined; }}
          {isSaving}
          showInitialBalance={false}
          submitLabel="Save"
          initialValues={editInitialValues}
        />
      {/key}
    </div>
  </Modal>

  <!-- Set Balance Modal -->
  {#if selectedAccount}
    <SetBalanceModal
      open={showSetBalanceModal}
      accountId={selectedAccount.account_id}
      accountName={getDisplayName(selectedAccount)}
      currentBalance={selectedAccount.balance}
      currentBalanceDate={selectedAccount.balance_as_of?.split("T")[0]}
      onclose={() => { showSetBalanceModal = false; }}
      onsave={handleSetBalanceSave}
    />
  {/if}

  <!-- CSV Import Modal -->
  <ImportModal
    open={showCsvImportModal}
    initialAccountId={selectedAccount?.account_id}
    onclose={() => { showCsvImportModal = false; }}
    onsuccess={handleCsvImportSuccess}
  />

  <!-- Recalculate Balances Modal -->
  {#if selectedAccount}
    <RecalculateBalancesModal
      open={showRecalculateModal}
      accountId={selectedAccount.account_id}
      accountName={getDisplayName(selectedAccount)}
      onclose={() => { showRecalculateModal = false; }}
      onsuccess={() => { showRecalculateModal = false; loadAccounts(); loadDetailData(selectedAccount.account_id); }}
    />
  {/if}

  <!-- Delete Confirmation -->
  <Modal open={showDeleteConfirm} title="Delete Account" onclose={() => { showDeleteConfirm = false; }} width="420px">
    <div class="modal-body">
      <p>Delete <strong>{selectedAccount?.name}</strong>?</p>

      <div class="delete-summary">
        <p>This will permanently delete:</p>
        <ul>
          {#if selectedAccount?.transaction_count && selectedAccount.transaction_count > 0}
            <li>{selectedAccount.transaction_count} transaction{selectedAccount.transaction_count === 1 ? '' : 's'}</li>
          {/if}
          <li>Balance history</li>
        </ul>
      </div>

      <p class="delete-warning">This cannot be undone.</p>
    </div>
    {#snippet actions()}
      <button class="btn secondary" onclick={() => { showDeleteConfirm = false; }}>Cancel</button>
      <button class="btn danger" onclick={handleDelete} disabled={isSaving}>
        {isSaving ? "Deleting..." : "Delete Account"}
      </button>
    {/snippet}
  </Modal>
</div>

<style>
  .accounts-view {
    height: 100%;
    display: flex;
    flex-direction: column;
    background: var(--bg-primary);
    color: var(--text-primary);
  }

  /* Header */
  .view-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: var(--spacing-md) var(--spacing-lg);
    background: var(--bg-secondary);
    border-bottom: 1px solid var(--border-primary);
  }

  .view-header h1 {
    margin: 0;
    font-size: 16px;
    font-weight: 600;
  }

  .btn-add {
    padding: 6px 12px;
    background: var(--accent-primary);
    border: none;
    border-radius: 4px;
    color: white;
    font-size: 12px;
    font-weight: 500;
    cursor: pointer;
  }

  .btn-add:hover {
    opacity: 0.9;
  }

  /* Loading */
  .loading {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--spacing-md);
    color: var(--text-muted);
  }

  .spinner {
    width: 24px;
    height: 24px;
    border: 2px solid var(--border-primary);
    border-top-color: var(--accent-primary);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  /* Empty State */
  .empty-state {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .empty-content {
    text-align: center;
    max-width: 320px;
  }

  .empty-content h2 {
    margin: 0 0 var(--spacing-sm);
    font-size: 18px;
    color: var(--text-primary);
  }

  .empty-content p {
    margin: 0 0 var(--spacing-lg);
    color: var(--text-muted);
    font-size: 14px;
  }

  .btn-primary {
    padding: 10px 20px;
    background: var(--accent-primary);
    border: none;
    border-radius: 6px;
    color: white;
    font-size: 14px;
    font-weight: 500;
    cursor: pointer;
  }

  .empty-actions {
    display: flex;
    gap: var(--spacing-sm);
    flex-wrap: wrap;
    justify-content: center;
  }

  .btn-secondary {
    padding: 10px 20px;
    background: var(--bg-tertiary);
    border: 1px solid var(--border-primary);
    border-radius: 6px;
    color: var(--text-primary);
    font-size: 14px;
    font-weight: 500;
    cursor: pointer;
  }

  .btn-secondary:hover {
    background: var(--bg-secondary);
  }

  /* Main Content */
  .main-content {
    flex: 1;
    display: flex;
    overflow: hidden;
  }

  /* Account List */
  .account-list {
    width: 360px;
    border-right: 1px solid var(--border-primary);
    overflow-y: auto;
  }

  .section {
    padding: var(--spacing-md) 0;
  }

  .section-header {
    padding: var(--spacing-xs) var(--spacing-lg);
    font-size: 11px;
    font-weight: 600;
    color: var(--text-muted);
    letter-spacing: 0.5px;
  }

  .row {
    width: 100%;
    display: flex;
    align-items: center;
    gap: var(--spacing-md);
    padding: var(--spacing-sm) var(--spacing-lg);
    background: transparent;
    border: none;
    text-align: left;
    cursor: pointer;
    transition: background 0.1s;
  }

  .row:hover {
    background: var(--bg-secondary);
  }

  .row.selected {
    background: var(--bg-tertiary);
  }

  .row.cursor {
    outline: 2px solid var(--accent-primary);
    outline-offset: -2px;
  }

  .row-info {
    flex: 1;
    min-width: 0;
  }

  .row-name {
    display: block;
    font-size: 13px;
    font-weight: 500;
    color: var(--text-primary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .row-subtitle {
    display: block;
    font-size: 11px;
    color: var(--text-muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .row-balance {
    font-size: 13px;
    font-weight: 500;
    font-family: var(--font-mono);
    color: var(--text-primary);
    white-space: nowrap;
  }

  .row-balance.negative {
    color: var(--accent-danger, #ef4444);
  }

  .row-balance.no-balance {
    color: var(--text-muted);
    font-style: italic;
  }

  /* Detail Panel */
  .detail-panel {
    flex: 1;
    overflow-y: auto;
    padding: var(--spacing-lg);
    display: flex;
    flex-direction: column;
    gap: var(--spacing-lg);
  }

  .detail-header {
    display: flex;
    align-items: baseline;
    gap: var(--spacing-sm);
  }

  .detail-name {
    font-size: 18px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .detail-institution {
    font-size: 14px;
    color: var(--text-muted);
  }

  .detail-balance {
    font-size: 32px;
    font-weight: 600;
    font-family: var(--font-mono);
    color: var(--text-primary);
  }

  .detail-balance.no-balance {
    display: flex;
    align-items: baseline;
    gap: var(--spacing-md);
    font-size: 18px;
    font-weight: 400;
    color: var(--text-muted);
    font-style: italic;
  }

  .detail-balance.no-balance .btn-link {
    font-size: 13px;
    font-style: normal;
    color: var(--accent-primary);
    background: none;
    border: none;
    padding: 0;
    cursor: pointer;
  }

  .detail-balance.no-balance .btn-link:hover {
    text-decoration: underline;
  }

  /* Chart Section */
  .chart-section {
    background: var(--bg-secondary);
    border: 1px solid var(--border-primary);
    border-radius: 8px;
    padding: var(--spacing-md);
  }

  .chart-label {
    font-size: 11px;
    font-weight: 600;
    color: var(--text-muted);
    letter-spacing: 0.5px;
    margin-bottom: var(--spacing-sm);
  }

  .chart-empty {
    padding: var(--spacing-xl) var(--spacing-md);
    text-align: center;
    color: var(--text-muted);
  }

  .chart-empty p {
    margin: 0;
    font-size: 13px;
  }

  .chart-empty .hint {
    margin-top: var(--spacing-xs);
    font-size: 12px;
    opacity: 0.8;
  }

  /* Action Buttons */
  .detail-actions {
    display: flex;
    gap: var(--spacing-sm);
    flex-wrap: wrap;
  }

  .action-btn {
    padding: 8px 14px;
    background: var(--bg-tertiary);
    border: 1px solid var(--border-primary);
    border-radius: 6px;
    color: var(--text-primary);
    font-size: 12px;
    font-weight: 500;
    cursor: pointer;
    transition: background 0.1s, border-color 0.1s;
  }

  .action-btn:hover {
    background: var(--bg-secondary);
    border-color: var(--text-muted);
  }

  /* Snapshots Section */
  .snapshots-section {
    background: var(--bg-secondary);
    border: 1px solid var(--border-primary);
    border-radius: 8px;
    padding: var(--spacing-md);
  }

  .snapshots-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: var(--spacing-sm);
  }

  .snapshots-label {
    font-size: 11px;
    font-weight: 600;
    color: var(--text-muted);
    letter-spacing: 0.5px;
  }

  .snapshots-actions {
    display: flex;
    gap: var(--spacing-xs);
  }

  .snapshots-btn {
    padding: 4px 10px;
    background: var(--bg-tertiary);
    border: 1px solid var(--border-primary);
    border-radius: 4px;
    color: var(--text-primary);
    font-size: 11px;
    font-weight: 500;
    cursor: pointer;
    transition: background 0.1s, border-color 0.1s;
  }

  .snapshots-btn:hover {
    background: var(--bg-primary);
    border-color: var(--text-muted);
  }

  .snapshots-table-wrapper {
    max-height: 300px;
    overflow-y: auto;
  }

  .snapshots-table {
    display: flex;
    flex-direction: column;
  }

  .snapshots-empty {
    padding: var(--spacing-lg) var(--spacing-md);
    text-align: center;
    color: var(--text-muted);
    font-size: 12px;
  }

  .snapshot-row {
    display: flex;
    align-items: center;
    gap: var(--spacing-md);
    padding: var(--spacing-xs) 0;
    font-size: 12px;
    border-bottom: 1px solid var(--border-primary);
  }

  .snapshot-row:last-child {
    border-bottom: none;
  }

  .snapshot-date {
    width: 80px;
    color: var(--text-muted);
  }

  .snapshot-balance {
    flex: 1;
    font-family: var(--font-mono);
    color: var(--text-primary);
  }

  .snapshot-source {
    width: 60px;
    color: var(--text-muted);
    font-size: 11px;
  }

  .snapshot-delete {
    width: 24px;
    height: 24px;
    padding: 4px;
    background: transparent;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    visibility: hidden;
    display: flex;
    align-items: center;
    justify-content: center;
    transition: color 0.1s;
  }

  .snapshot-row:hover .snapshot-delete {
    visibility: visible;
  }

  .snapshot-row:hover .snapshot-delete:hover {
    color: var(--accent-danger, #ef4444);
  }

  /* Metadata */
  .detail-meta {
    font-size: 11px;
    color: var(--text-muted);
    padding-top: var(--spacing-sm);
    border-top: 1px solid var(--border-primary);
  }

  /* Modal Body */
  .modal-body {
    padding: var(--spacing-lg);
  }

  .modal-body p {
    margin: 0 0 var(--spacing-sm);
    font-size: 14px;
    color: var(--text-primary);
  }

  .delete-summary {
    margin: var(--spacing-md) 0;
    padding: var(--spacing-md);
    background: var(--bg-tertiary);
    border-radius: 6px;
  }

  .delete-summary p {
    margin: 0 0 var(--spacing-sm) 0;
    font-size: 13px;
    color: var(--text-secondary);
  }

  .delete-summary ul {
    margin: 0;
    padding-left: var(--spacing-lg);
  }

  .delete-summary li {
    font-size: 13px;
    color: var(--text-primary);
    margin: 4px 0;
  }

  .delete-warning {
    color: var(--text-muted) !important;
    font-size: 13px !important;
  }
</style>
