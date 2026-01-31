<script lang="ts">
  /**
   * ImportModal - Global modal for importing transactions from CSV files
   * Includes account selection/creation and column mapping
   */
  import { Modal, Icon, formatUserCurrency, AddOrUpdateAccountForm, type AddAccountFormData } from "../shared";
  import {
    pickCsvFile,
    getCsvHeaders,
    importCsvPreview,
    importCsvExecute,
    executeQuery,
    getDemoMode,
    listImportProfiles,
    getImportProfile,
    saveImportProfile,
    getAccountProfileMapping,
    setAccountProfileMapping,
    moveImportedFile,
    toast,
    backfillExecute,
    type ImportColumnMapping,
    type ImportPreviewResult,
    type ImportExecuteResult,
    type ImportProfile,
    type NumberFormat,
  } from "../sdk";
  import { getUserCurrencySymbol } from "../shared";
  import { applyRulesToBatch } from "../plugins/transactions/rules";
  import { loadAccountsWithClassification, type AccountBasicInfo } from "../plugins/accounts/types";

  interface Props {
    open: boolean;
    onclose: () => void;
    onsuccess: (batchId: string) => void;
    initialAccountId?: string;
    initialFilePath?: string | null;
  }

  let { open, onclose, onsuccess, initialAccountId, initialFilePath }: Props = $props();

  // Step tracking
  type Step = "account" | "file" | "mapping" | "done";
  let currentStep = $state<Step>("account");

  // Account state
  let accounts = $state<AccountBasicInfo[]>([]);
  let selectedAccountId = $state("");
  let isLoadingAccounts = $state(false);
  let showCreateAccount = $state(false);
  let isCreatingAccount = $state(false);

  // Import state
  let filePath = $state("");
  let fileName = $state("");
  let headers = $state<string[]>([]);
  let columnMapping = $state<ImportColumnMapping>({});
  let flipSigns = $state(false);
  let debitNegative = $state(false);
  let skipRows = $state(0);
  let numberFormat = $state<NumberFormat>("us");
  let useSplitAmounts = $state(false); // Toggle for debit/credit mode
  let preview = $state<ImportPreviewResult | null>(null);
  let result = $state<ImportExecuteResult | null>(null);
  let error = $state<string | null>(null);
  let isImporting = $state(false);
  let isLoadingPreview = $state(false);
  let isUndoing = $state(false);
  let previewDebounceTimer: ReturnType<typeof setTimeout> | null = null;
  let demoModeWarning = $state(false);

  // Profile state
  let availableProfiles = $state<string[]>([]);
  let selectedProfile = $state("");
  let saveProfileName = $state("");
  let isSavingProfile = $state(false);
  let saveAccountMapping = $state(false);
  let accountMappedProfile = $state<string | null>(null);

  // Statement balance state (for auto-backfill)
  let statementBalance = $state("");
  let statementBalanceDate = $state("");
  let backfillSnapshotsCreated = $state(0);
  let hasUserSetBalanceDate = $state(false);
  let hasCheckedExistingSnapshot = $state(false);

  // Selected account info for display
  let selectedAccount = $derived(accounts.find(a => a.id === selectedAccountId));

  // Derived: latest transaction date from preview (empty string if no preview yet)
  let latestTransactionDate = $derived.by(() => {
    if (!preview?.preview?.length) return "";
    return preview.preview.reduce((max, txn) =>
      txn.date > max ? txn.date : max,
      preview.preview[0].date
    );
  });

  // Auto-update statement date when preview loads (if not already set by user)
  $effect(() => {
    if (latestTransactionDate && !hasUserSetBalanceDate && currentStep === "mapping") {
      statementBalanceDate = latestTransactionDate;
    }
  });

  // Check for existing balance snapshot on the statement date (for re-imports)
  async function checkExistingSnapshot() {
    if (!statementBalanceDate || !selectedAccountId || hasCheckedExistingSnapshot) return;

    try {
      const res = await executeQuery(`
        SELECT balance
        FROM sys_balance_snapshots
        WHERE account_id = '${selectedAccountId}'
          AND DATE(snapshot_time) = '${statementBalanceDate}'
        ORDER BY snapshot_time DESC
        LIMIT 1
      `);

      if (res.rows.length > 0 && !statementBalance) {
        const existingBalance = res.rows[0][0] as number;
        statementBalance = existingBalance.toFixed(2);
      }
      hasCheckedExistingSnapshot = true;
    } catch (e) {
      // Ignore errors - just won't pre-fill
    }
  }

  // Check for existing snapshot when date changes
  $effect(() => {
    if (statementBalanceDate && selectedAccountId && currentStep === "mapping" && !hasCheckedExistingSnapshot) {
      checkExistingSnapshot();
    }
  });

  // Date range of imported transactions (fetched after import)
  let importedDateRange = $state<string | null>(null);

  // Whether balance column should show in preview (from CSV column OR calculated from anchor)
  let showBalanceInPreview = $derived(
    !!columnMapping.balanceColumn ||
    (preview?.preview?.some(txn => txn.balance != null) ?? false)
  );

  function formatDate(dateStr: string): string {
    try {
      const date = new Date(dateStr);
      return date.toLocaleDateString("en-US", { month: "short", day: "numeric", year: "numeric" });
    } catch {
      return dateStr;
    }
  }

  async function fetchImportedDateRange(batchId: string) {
    try {
      const res = await executeQuery(`
        SELECT MIN(transaction_date) as min_date, MAX(transaction_date) as max_date
        FROM sys_transactions
        WHERE csv_batch_id = '${batchId}'
      `);
      if (res.rows && res.rows.length > 0) {
        const minDate = res.rows[0][0] as string;
        const maxDate = res.rows[0][1] as string;
        if (minDate && maxDate) {
          if (minDate === maxDate) {
            importedDateRange = formatDate(minDate);
          } else {
            importedDateRange = `${formatDate(minDate)} – ${formatDate(maxDate)}`;
          }
        }
      }
    } catch (e) {
      console.error("Failed to fetch date range:", e);
    }
  }

  // Reset state when modal opens
  $effect(() => {
    if (open) {
      resetState();
      loadAccounts().then(() => {
        // If we have an initial file path (from drag-drop), load it automatically
        if (initialFilePath) {
          loadInitialFile();
        }
      });
      loadProfiles();
      checkDemoMode();
    }
  });

  async function loadProfiles() {
    try {
      availableProfiles = await listImportProfiles();
    } catch (e) {
      console.error("Failed to load profiles:", e);
      availableProfiles = [];
    }
  }

  async function applyProfile(profileName: string) {
    if (!profileName) {
      // Re-run auto-detection when switching back to "Auto-detect"
      selectedProfile = "";
      if (headers.length > 0) {
        columnMapping = autoDetectColumns(headers);
        flipSigns = false;
        debitNegative = false;
        useSplitAmounts = !!(columnMapping.debitColumn || columnMapping.creditColumn);
      }
      return;
    }

    try {
      const profile = await getImportProfile(profileName);
      if (profile) {
        // Apply column mappings
        columnMapping = {
          dateColumn: profile.columnMappings.date,
          amountColumn: profile.columnMappings.amount,
          descriptionColumn: profile.columnMappings.description,
          debitColumn: profile.columnMappings.debit,
          creditColumn: profile.columnMappings.credit,
          balanceColumn: profile.columnMappings.balance,
        };
        // Determine if split amounts mode
        useSplitAmounts = !!(profile.columnMappings.debit || profile.columnMappings.credit);
        // Apply options
        flipSigns = profile.options.flipSigns || false;
        debitNegative = profile.options.debitNegative || false;
        skipRows = profile.options.skipRows || 0;
        numberFormat = profile.options.numberFormat || "us";
        selectedProfile = profileName;

        // Re-fetch headers if skipRows changed
        if (filePath && profile.options.skipRows) {
          try {
            headers = await getCsvHeaders(filePath, profile.options.skipRows);
          } catch (e) {
            console.error("Failed to re-fetch headers with skip rows:", e);
          }
        }
      }
    } catch (e) {
      console.error("Failed to apply profile:", e);
    }
  }

  /**
   * Load and apply the account's mapped profile, or apply liability default.
   * This is the single source of truth for profile application when entering the mapping step.
   */
  async function loadAndApplyAccountProfile() {
    if (!selectedAccountId) return;

    // Load the account's mapped profile
    try {
      accountMappedProfile = await getAccountProfileMapping(selectedAccountId);
    } catch (e) {
      console.error("Failed to load account profile mapping:", e);
      accountMappedProfile = null;
    }

    // Apply the profile, or fall back to liability default
    if (accountMappedProfile) {
      await applyProfile(accountMappedProfile);
    } else if (selectedAccount?.classification === 'liability') {
      flipSigns = true;
    }
  }

  async function handleSaveProfile() {
    if (!saveProfileName.trim()) return;

    isSavingProfile = true;
    try {
      const profileName = saveProfileName.trim();

      await saveImportProfile(
        profileName,
        {
          date: columnMapping.dateColumn,
          amount: columnMapping.amountColumn,
          description: columnMapping.descriptionColumn,
          debit: columnMapping.debitColumn,
          credit: columnMapping.creditColumn,
          balance: columnMapping.balanceColumn,
        },
        {
          flipSigns,
          debitNegative,
          skipRows,
          numberFormat,
        }
      );

      // Save account mapping if checkbox is checked
      if (saveAccountMapping && selectedAccountId) {
        await setAccountProfileMapping(selectedAccountId, profileName);
      }

      // Refresh profiles list
      await loadProfiles();

      // Show success toast
      const message = saveAccountMapping && selectedAccount
        ? `Profile "${profileName}" saved and set as default for ${selectedAccount.name}`
        : `Profile "${profileName}" saved`;
      toast.success(message);

      saveProfileName = "";
      saveAccountMapping = false;
    } catch (e) {
      error = e instanceof Error ? e.message : "Failed to save profile";
    } finally {
      isSavingProfile = false;
    }
  }

  async function loadInitialFile() {
    if (!initialFilePath) return;

    try {
      headers = await getCsvHeaders(initialFilePath);
      columnMapping = autoDetectColumns(headers);
      useSplitAmounts = !!(columnMapping.debitColumn || columnMapping.creditColumn);
      // If we have both account and file ready, skip to mapping
      // Otherwise stay on account selection
      if (selectedAccountId) {
        currentStep = "mapping";
        await loadAndApplyAccountProfile();
      }
    } catch (e) {
      error = e instanceof Error ? e.message : "Failed to read CSV headers";
    }
  }

  function resetState() {
    currentStep = "account";
    selectedAccountId = initialAccountId || "";
    showCreateAccount = false;
    isCreatingAccount = false;
    // If we have an initial file path from drag-drop, use it
    if (initialFilePath) {
      filePath = initialFilePath;
      fileName = initialFilePath.split("/").pop() || "";
    } else {
      filePath = "";
      fileName = "";
    }
    headers = [];
    columnMapping = {};
    flipSigns = false;
    debitNegative = false;
    skipRows = 0;
    numberFormat = "us";
    useSplitAmounts = false;
    preview = null;
    result = null;
    error = null;
    importedDateRange = null;
    isImporting = false;
    isLoadingPreview = false;
    // Reset profile state
    selectedProfile = "";
    saveProfileName = "";
    saveAccountMapping = false;
    accountMappedProfile = null;
    // Reset statement balance state
    statementBalance = "";
    statementBalanceDate = "";
    backfillSnapshotsCreated = 0;
    hasUserSetBalanceDate = false;
    hasCheckedExistingSnapshot = false;
  }

  async function checkDemoMode() {
    demoModeWarning = await getDemoMode();
  }

  async function loadAccounts() {
    isLoadingAccounts = true;
    try {
      accounts = await loadAccountsWithClassification();

      // If initialAccountId is set and valid, skip to file step
      // Use proceedToFile() to ensure account's mapped profile is loaded
      if (initialAccountId && accounts.some(a => a.id === initialAccountId)) {
        selectedAccountId = initialAccountId;
        await proceedToFile();
      }
    } catch (e) {
      error = e instanceof Error ? e.message : "Failed to load accounts";
    } finally {
      isLoadingAccounts = false;
    }
  }

  async function handleCreateAccount(formData: AddAccountFormData) {
    isCreatingAccount = true;
    error = null;

    try {
      const accountId = crypto.randomUUID();
      const now = new Date().toISOString();
      const typeValue = formData.account_type.trim() || null;
      const nicknameValue = formData.nickname.trim() || null;

      await executeQuery(
        `INSERT INTO sys_accounts (account_id, name, nickname, account_type, classification, institution_name, currency, balance, external_ids, is_manual, created_at, updated_at)
         VALUES ('${accountId}', '${formData.name.trim().replace(/'/g, "''")}', ${nicknameValue ? `'${nicknameValue.replace(/'/g, "''")}'` : 'NULL'}, ${typeValue ? `'${typeValue}'` : 'NULL'},
                 '${formData.classification}',
                 ${formData.institution_name.trim() ? `'${formData.institution_name.trim().replace(/'/g, "''")}'` : 'NULL'},
                 'USD', 0, '{}', TRUE, '${now}', '${now}')`,
        { readonly: false }
      );

      // If initial balance is specified, create a balance snapshot
      if (formData.initial_balance.trim()) {
        const initialBalance = parseFloat(formData.initial_balance);
        if (!isNaN(initialBalance)) {
          const snapshotId = crypto.randomUUID();
          await executeQuery(
            `INSERT INTO sys_balance_snapshots (snapshot_id, account_id, balance, snapshot_time, source, created_at, updated_at)
             VALUES ('${snapshotId}', '${accountId}', ${initialBalance}, '${now}', 'manual', '${now}', '${now}')`,
            { readonly: false }
          );
        }
      }

      // Refresh accounts and select the new one
      await loadAccounts();
      selectedAccountId = accountId;
      showCreateAccount = false;
    } catch (e) {
      error = e instanceof Error ? e.message : "Failed to create account";
    } finally {
      isCreatingAccount = false;
    }
  }

  async function proceedToFile() {
    if (!selectedAccountId) {
      error = "Please select an account";
      return;
    }
    error = null;

    // If file is already loaded (from drag-drop), skip to mapping
    if (filePath && headers.length > 0) {
      currentStep = "mapping";
      await loadAndApplyAccountProfile();
    } else {
      currentStep = "file";
    }
  }

  // Auto-update preview when mapping or options change
  // Note: skipRows is NOT included here - handleSkipRowsChange handles that case
  // to avoid race condition between fetching new headers and triggering preview
  $effect(() => {
    if (filePath && selectedAccountId && open && currentStep === "mapping") {
      const _mapping = JSON.stringify(columnMapping);
      const _flip = flipSigns;
      const _debit = debitNegative;
      const _format = numberFormat;
      const _anchorBalance = statementBalance;
      const _anchorDate = statementBalanceDate;
      debouncedPreview();
    }
  });

  function debouncedPreview() {
    if (previewDebounceTimer) {
      clearTimeout(previewDebounceTimer);
    }
    previewDebounceTimer = setTimeout(() => {
      loadPreview();
    }, 300);
  }

  function getErrorGuidance(errorMsg: string): string | null {
    if (errorMsg.includes("column") && errorMsg.includes("not found")) {
      return "The selected column doesn't exist in your CSV. Please select the correct columns from the dropdowns above.";
    }
    if (errorMsg.includes("skip_rows") || errorMsg.includes("No header row")) {
      return "Try adjusting 'Skip rows' to skip any non-data rows at the top of your file.";
    }
    return null;
  }

  async function loadPreview() {
    if (!filePath || !selectedAccountId) return;

    // Don't try to preview if required columns aren't selected
    if (!columnMapping.dateColumn) {
      error = null;
      preview = null;
      return;
    }
    if (!columnMapping.amountColumn && !columnMapping.debitColumn && !columnMapping.creditColumn) {
      error = null;
      preview = null;
      return;
    }

    isLoadingPreview = true;
    try {
      // Parse anchor balance if provided (for balance calculation in preview)
      const anchorBalanceNum = statementBalance ? parseFloat(statementBalance) : undefined;
      const anchorDateStr = statementBalanceDate || undefined;

      preview = await importCsvPreview(
        filePath,
        selectedAccountId,
        columnMapping,
        flipSigns,
        debitNegative,
        skipRows,
        numberFormat,
        !isNaN(anchorBalanceNum ?? NaN) ? anchorBalanceNum : undefined,
        anchorDateStr
      );
      error = null;
    } catch (e) {
      const msg = e instanceof Error ? e.message : "Failed to preview CSV";
      error = msg;
      preview = null;
    } finally {
      isLoadingPreview = false;
    }
  }

  async function handleSkipRowsChange() {
    if (!filePath) return;

    try {
      headers = await getCsvHeaders(filePath, skipRows);
      columnMapping = autoDetectColumns(headers);
      useSplitAmounts = !!(columnMapping.debitColumn || columnMapping.creditColumn);
    } catch (e) {
      error = e instanceof Error ? e.message : "Failed to read CSV headers";
    }
  }

  async function handleFileSelect() {
    const path = await pickCsvFile();
    if (!path) return;

    filePath = path;
    fileName = path.split("/").pop() || path;

    try {
      headers = await getCsvHeaders(path, skipRows);
      columnMapping = autoDetectColumns(headers);
      useSplitAmounts = !!(columnMapping.debitColumn || columnMapping.creditColumn);
      currentStep = "mapping";
      await loadAndApplyAccountProfile();
    } catch (e) {
      error = e instanceof Error ? e.message : "Failed to read CSV headers";
    }
  }

  function autoDetectColumns(headers: string[]): ImportColumnMapping {
    const mapping: ImportColumnMapping = {};
    const lowerHeaders = headers.map((h) => h.toLowerCase());

    // Date detection
    const datePatterns = ["date", "transaction date", "posted", "trans date"];
    for (const pattern of datePatterns) {
      const idx = lowerHeaders.findIndex((h) => h.includes(pattern));
      if (idx >= 0) {
        mapping.dateColumn = headers[idx];
        break;
      }
    }

    // Description detection
    const descPatterns = ["description", "desc", "memo", "narrative", "details"];
    for (const pattern of descPatterns) {
      const idx = lowerHeaders.findIndex((h) => h.includes(pattern));
      if (idx >= 0) {
        mapping.descriptionColumn = headers[idx];
        break;
      }
    }

    // Amount detection
    const amountPatterns = ["amount", "total"];
    for (const pattern of amountPatterns) {
      const idx = lowerHeaders.findIndex(
        (h) => h.includes(pattern) && !h.includes("debit") && !h.includes("credit")
      );
      if (idx >= 0) {
        mapping.amountColumn = headers[idx];
        break;
      }
    }

    // Debit/Credit detection
    if (!mapping.amountColumn) {
      const debitIdx = lowerHeaders.findIndex((h) => h.includes("debit"));
      const creditIdx = lowerHeaders.findIndex((h) => h.includes("credit"));
      if (debitIdx >= 0) mapping.debitColumn = headers[debitIdx];
      if (creditIdx >= 0) mapping.creditColumn = headers[creditIdx];
    }

    // Balance detection (running balance / balance after transaction)
    const balancePatterns = ["balance", "saldo", "running"];
    for (const pattern of balancePatterns) {
      const idx = lowerHeaders.findIndex((h) => h.includes(pattern));
      if (idx >= 0) {
        mapping.balanceColumn = headers[idx];
        break;
      }
    }

    return mapping;
  }

  function handleSplitAmountsToggle() {
    useSplitAmounts = !useSplitAmounts;
    if (useSplitAmounts) {
      // Switching to split mode - clear single amount
      columnMapping.amountColumn = "";
    } else {
      // Switching to single mode - clear debit/credit
      columnMapping.debitColumn = "";
      columnMapping.creditColumn = "";
    }
  }

  async function handleImportExecute() {
    if (!filePath || !selectedAccountId) return;

    isImporting = true;
    error = null;

    try {
      result = await importCsvExecute(
        filePath,
        selectedAccountId,
        columnMapping,
        flipSigns,
        debitNegative,
        skipRows,
        numberFormat
      );
      currentStep = "done";
      // Fetch the actual date range from imported transactions
      if (result.batch_id) {
        await fetchImportedDateRange(result.batch_id);
        // Apply auto-tag rules to newly imported transactions
        try {
          const tagged = await applyRulesToBatch(result.batch_id);
          if (tagged > 0) {
            console.log(`Auto-tagged ${tagged} transactions`);
          }
        } catch (e) {
          console.error("Failed to apply auto-tag rules:", e);
          // Non-fatal - don't block the success flow
        }
      }
      // Move file to imported folder if it was from the watch folder
      if (filePath.includes(".treeline/imports/") && !filePath.includes("/imported/")) {
        try {
          await moveImportedFile(filePath);
        } catch (e) {
          console.error("Failed to move imported file:", e);
          // Non-fatal - don't block the success flow
        }
      }

      // If user provided a statement balance, run backfill to calculate historical balances
      const balance = parseFloat(statementBalance);
      if (!isNaN(balance) && statementBalanceDate) {
        try {
          const backfillResult = await backfillExecute(
            selectedAccountId,
            balance,
            statementBalanceDate
          );
          backfillSnapshotsCreated = backfillResult.snapshots_created;
        } catch (backfillErr) {
          console.error("Backfill failed:", backfillErr);
          // Non-fatal - don't block the success flow
        }
      }
    } catch (e) {
      error = e instanceof Error ? e.message : "Import failed";
    } finally {
      isImporting = false;
    }
  }

  async function handleUndoImport() {
    if (!result?.batch_id) return;

    isUndoing = true;
    error = null;

    try {
      // Delete all transactions with this batch_id
      await executeQuery(
        `DELETE FROM sys_transactions WHERE csv_batch_id = '${result.batch_id}'`,
        { readonly: false }
      );

      // Reset and close
      onclose();
    } catch (e) {
      error = e instanceof Error ? e.message : "Failed to undo import";
    } finally {
      isUndoing = false;
    }
  }

  function handleClose() {
    if (result) {
      onsuccess(result.batch_id);
    }
    onclose();
  }

  function handleChangeFile() {
    filePath = "";
    headers = [];
    columnMapping = {};
    preview = null;
    currentStep = "file";
  }

  function handleChangeAccount() {
    currentStep = "account";
    filePath = "";
    headers = [];
    columnMapping = {};
    preview = null;
  }
</script>

<Modal
  {open}
  title="Import CSV"
  onclose={handleClose}
  width="720px"
>
  <div class="import-body">
    {#if demoModeWarning}
      <div class="import-demo-warning">
        <span class="warning-icon"><Icon name="beaker" size={16} /></span>
        <div class="warning-content">
          <strong>Demo Mode Active</strong>
          <p>This data will be imported to the demo database, not your real data.</p>
        </div>
      </div>
    {/if}

    {#if error}
      <div class="import-error">
        <div class="error-message">{error}</div>
        {#if getErrorGuidance(error)}
          <div class="error-guidance">{getErrorGuidance(error)}</div>
        {/if}
      </div>
    {/if}

    {#if currentStep === "account"}
      <!-- Step 1: Select Account -->
      <div class="import-step">
        <p class="step-title">Select Account</p>
        <p class="step-hint">Choose where to import the transactions.</p>

        {#if filePath}
          <p class="preselected-file">
            <Icon name="file-text" size={12} />
            Importing: <strong>{fileName}</strong>
          </p>
        {/if}

        {#if isLoadingAccounts}
          <div class="loading">Loading accounts...</div>
        {:else if showCreateAccount}
          <div class="create-account-form">
            <AddOrUpdateAccountForm
              onsubmit={handleCreateAccount}
              oncancel={() => showCreateAccount = false}
              isSaving={isCreatingAccount}
            />
          </div>
        {:else}
          <div class="account-list">
            {#each accounts as account}
              <button
                class="account-option"
                class:selected={selectedAccountId === account.id}
                onclick={() => selectedAccountId = account.id}
              >
                <div class="account-info">
                  <span class="account-name">{account.name}</span>
                  {#if account.institution_name}
                    <span class="account-institution">{account.institution_name}</span>
                  {/if}
                </div>
                <span class="account-classification" class:liability={account.classification === 'liability'}>
                  {account.classification === 'liability' ? 'Liability' : 'Asset'}
                </span>
              </button>
            {/each}
            <button class="account-option create-new" onclick={() => showCreateAccount = true}>
              <Icon name="plus" size={14} />
              <span>Create New Account</span>
            </button>
          </div>
        {/if}
      </div>

    {:else if currentStep === "file"}
      <!-- Step 2: Select File -->
      <div class="import-step">
        <div class="selected-account-bar">
          <span class="label">Account:</span>
          <span class="value">{selectedAccount?.name}</span>
          <button class="btn-link" onclick={handleChangeAccount}>Change</button>
        </div>

        <p class="step-title">Select CSV File</p>
        <p class="step-hint">Choose a CSV file exported from your bank.</p>

        <button class="file-select-btn" onclick={handleFileSelect}>
          Select CSV File...
        </button>
      </div>

    {:else if currentStep === "mapping"}
      <!-- Step 3: Column Mapping -->
      <div class="import-step">
        <!-- Context bar -->
        <div class="context-bar">
          <div class="context-item">
            <span class="context-label">Account:</span>
            <span class="context-value">{selectedAccount?.name}</span>
            <span class="context-classification" class:liability={selectedAccount?.classification === 'liability'}>
              {selectedAccount?.classification === 'liability' ? 'Liability' : 'Asset'}
            </span>
          </div>
          <div class="context-item">
            <span class="context-label">File:</span>
            <span class="context-value file-value">{fileName}</span>
            <button class="btn-link" onclick={handleChangeFile}>Change</button>
          </div>
        </div>

        <!-- Profile & Skip Rows (combined pre-processing) -->
        <div class="preprocessing-section">
          <div class="preprocessing-row">
            {#if availableProfiles.length > 0 || accountMappedProfile}
              <div class="preprocessing-item profile-item">
                <label for="profile-select">Profile:</label>
                <select
                  id="profile-select"
                  value={selectedProfile}
                  onchange={(e) => applyProfile(e.currentTarget.value)}
                >
                  <option value="">Auto-detect</option>
                  {#each availableProfiles as profile}
                    <option value={profile}>
                      {profile}{profile === accountMappedProfile ? " ★" : ""}
                    </option>
                  {/each}
                </select>
              </div>
            {/if}
            <div class="preprocessing-item skip-item">
              <label for="skip-rows">Skip rows:</label>
              <input
                id="skip-rows"
                type="number"
                min="0"
                max="100"
                bind:value={skipRows}
                oninput={handleSkipRowsChange}
              />
            </div>
          </div>
        </div>

        <!-- Column Mapping -->
        <div class="section">
          <div class="section-title">Columns</div>

          <div class="mapping-grid">
            <label for="date-col">Date</label>
            <select id="date-col" bind:value={columnMapping.dateColumn}>
              <option value="">-- Select --</option>
              {#each headers as header}
                <option value={header}>{header}</option>
              {/each}
            </select>

            <label for="desc-col">Description</label>
            <select id="desc-col" bind:value={columnMapping.descriptionColumn}>
              <option value="">-- Select --</option>
              {#each headers as header}
                <option value={header}>{header}</option>
              {/each}
            </select>

            {#if !useSplitAmounts}
              <label for="amount-col">Amount</label>
              <div class="amount-with-toggle">
                <select id="amount-col" bind:value={columnMapping.amountColumn}>
                  <option value="">-- Select --</option>
                  {#each headers as header}
                    <option value={header}>{header}</option>
                  {/each}
                </select>
                <button class="toggle-link" onclick={handleSplitAmountsToggle}>
                  Use debit/credit
                </button>
              </div>
            {:else}
              <label for="debit-column-select">Debit</label>
              <div class="split-amount-row">
                <select id="debit-column-select" bind:value={columnMapping.debitColumn}>
                  <option value="">-- Select --</option>
                  {#each headers as header}
                    <option value={header}>{header}</option>
                  {/each}
                </select>
              </div>

              <label for="credit-column-select">Credit</label>
              <div class="split-amount-row">
                <select id="credit-column-select" bind:value={columnMapping.creditColumn}>
                  <option value="">-- Select --</option>
                  {#each headers as header}
                    <option value={header}>{header}</option>
                  {/each}
                </select>
                <button class="toggle-link" onclick={handleSplitAmountsToggle}>
                  Use single amount
                </button>
              </div>
            {/if}

            <label for="balance-col">Balance <span class="optional">(optional)</span></label>
            <select id="balance-col" bind:value={columnMapping.balanceColumn}>
              <option value="">-- None --</option>
              {#each headers as header}
                <option value={header}>{header}</option>
              {/each}
            </select>
          </div>
        </div>

        <!-- Format Options -->
        <div class="section">
          <div class="section-title">Format</div>

          <div class="format-options">
            <div class="format-row">
              <label for="number-format">Numbers:</label>
              <select id="number-format" bind:value={numberFormat}>
                <option value="us">US: 1,234.56</option>
                <option value="eu">EU: 1.234,56</option>
                <option value="eu_space">EU: 1 234,56</option>
              </select>
            </div>

            <div class="format-checkboxes">
              <label class="checkbox-label">
                <input type="checkbox" bind:checked={flipSigns} />
                Flip signs <span class="checkbox-hint">(if charges show as positive)</span>
              </label>
              <label class="checkbox-label">
                <input type="checkbox" bind:checked={debitNegative} />
                Negate debits <span class="checkbox-hint">(if debits show as positive)</span>
              </label>
            </div>
          </div>
        </div>

        <!-- Anchor Balance - only show when CSV doesn't have a balance column -->
        {#if columnMapping.dateColumn && (columnMapping.amountColumn || columnMapping.debitColumn) && !columnMapping.balanceColumn}
          <div class="section anchor-balance-section">
            <div class="section-header">
              <div class="section-title">Anchor Balance <span class="optional">(optional)</span></div>
            </div>
            <p class="section-hint">
              {#if !statementBalance}
                Without an anchor, only transactions will be imported. Add a balance to see historical account balances.
              {:else}
                Treeline will calculate your balance for each day. See the Balance column in preview below.
              {/if}
            </p>
            <div class="balance-inputs">
              <div class="balance-input-group">
                <label for="anchor-balance">Ending balance</label>
                <div class="input-with-prefix">
                  <span class="input-prefix">{getUserCurrencySymbol()}</span>
                  <input
                    id="anchor-balance"
                    type="text"
                    inputmode="decimal"
                    placeholder="0.00"
                    bind:value={statementBalance}
                    oninput={(e) => {
                      statementBalance = e.currentTarget.value.replace(/[^0-9.-]/g, '').replace(/(\..*)\./g, '$1');
                    }}
                  />
                </div>
              </div>
              <div class="balance-input-group">
                <label for="anchor-date">As of date</label>
                <input
                  id="anchor-date"
                  type="date"
                  bind:value={statementBalanceDate}
                  onchange={() => { hasUserSetBalanceDate = true; hasCheckedExistingSnapshot = false; }}
                />
              </div>
            </div>
            {#if selectedAccount?.classification === 'liability'}
              <p class="credit-card-hint">
                This account is a liability. If you owe {formatUserCurrency(500)}, enter <strong>-500</strong>.
              </p>
            {/if}
          </div>
        {/if}

        <!-- Preview -->
        {#if columnMapping.dateColumn && (columnMapping.amountColumn || columnMapping.debitColumn || columnMapping.creditColumn)}
          <div class="section preview-section">
            <div class="section-header">
              <div class="section-title">Preview</div>
              {#if isLoadingPreview}
                <span class="preview-loading">Loading...</span>
              {/if}
            </div>

            {#if preview && preview.preview.length > 0}
              <div class="preview-table" class:with-balance={showBalanceInPreview}>
                <div class="preview-row header">
                  <span class="preview-date">Date</span>
                  <span class="preview-desc">Description</span>
                  <span class="preview-amount">Amount</span>
                  {#if showBalanceInPreview}
                    <span class="preview-balance">Balance</span>
                  {/if}
                </div>
                <div class="preview-body">
                  {#each preview.preview.slice(0, 100) as txn}
                    <div class="preview-row">
                      <span class="preview-date">{txn.date}</span>
                      <span class="preview-desc">{txn.description || ""}</span>
                      <span class="preview-amount" class:negative={txn.amount < 0}>
                        {formatUserCurrency(txn.amount)}
                      </span>
                      {#if showBalanceInPreview}
                        <span class="preview-balance">
                          {txn.balance != null ? formatUserCurrency(txn.balance) : "—"}
                        </span>
                      {/if}
                    </div>
                  {/each}
                </div>
              </div>
              <div class="preview-legend">
                <span class="negative">Red = spending</span>
                <span class="positive">Green = income</span>
                {#if preview.preview.length > 100}
                  <span class="preview-count">Showing 100 of {preview.preview.length}</span>
                {/if}
              </div>
            {:else if !isLoadingPreview}
              <div class="preview-empty">Select columns to see preview</div>
            {/if}
          </div>
        {/if}
      </div>

    {:else if currentStep === "done"}
      <!-- Step 4: Done -->
      <div class="import-done">
        <div class="done-header">
          <div class="done-icon"><Icon name="check" size={24} /></div>
          <div class="done-text">
            <p class="done-message">Imported {result?.imported} transactions</p>
            {#if result?.skipped && result.skipped > 0}
              <p class="done-skipped">{result.skipped} duplicates skipped</p>
            {/if}
            {#if (result?.balance_snapshots_created && result.balance_snapshots_created > 0) || backfillSnapshotsCreated > 0}
              {@const totalSnapshots = (result?.balance_snapshots_created || 0) + backfillSnapshotsCreated}
              <p class="done-snapshots">{totalSnapshots} balance snapshot{totalSnapshots > 1 ? 's' : ''} created</p>
            {/if}
          </div>
        </div>

        <div class="done-details">
          <div class="done-detail">
            <span class="done-label">File</span>
            <span class="done-value">{fileName}</span>
          </div>
          <div class="done-detail">
            <span class="done-label">Account</span>
            <span class="done-value">{selectedAccount?.name}</span>
          </div>
          {#if importedDateRange}
            <div class="done-detail">
              <span class="done-label">Date Range</span>
              <span class="done-value">{importedDateRange}</span>
            </div>
          {/if}
        </div>

        <button class="undo-btn" onclick={handleUndoImport} disabled={isUndoing}>
          <Icon name="refresh" size={14} />
          {isUndoing ? "Undoing..." : "Undo Import"}
        </button>

        <div class="save-profile-section">
          <div class="save-profile-header">Save as profile for future imports?</div>
          <div class="save-profile-input">
            <input
              type="text"
              placeholder="Profile name (e.g., chase, amex)"
              bind:value={saveProfileName}
              onkeydown={(e) => e.key === "Enter" && handleSaveProfile()}
            />
            <button
              class="btn-save-profile"
              onclick={handleSaveProfile}
              disabled={!saveProfileName.trim() || isSavingProfile}
            >
              {isSavingProfile ? "Saving..." : "Save"}
            </button>
          </div>
          {#if saveProfileName.trim() && selectedAccount}
            <label class="account-mapping-checkbox">
              <input type="checkbox" bind:checked={saveAccountMapping} />
              Auto-use this profile for "{selectedAccount.name}"
            </label>
          {/if}
        </div>
      </div>
    {/if}
  </div>

  {#snippet actions()}
    {#if currentStep === "account"}
      <button class="btn secondary" onclick={handleClose}>Cancel</button>
      {#if !showCreateAccount}
        <button class="btn primary" onclick={proceedToFile} disabled={!selectedAccountId}>
          Continue
        </button>
      {/if}
    {:else if currentStep === "file"}
      <button class="btn secondary" onclick={handleClose}>Cancel</button>
    {:else if currentStep === "mapping"}
      <button class="btn secondary" onclick={handleClose}>Cancel</button>
      {#if isImporting}
        <button class="btn primary" disabled>Importing...</button>
      {:else if preview && preview.preview.length > 0}
        <button class="btn primary" onclick={handleImportExecute}>Import</button>
      {/if}
    {:else if currentStep === "done"}
      <button class="btn primary" onclick={handleClose}>Done</button>
    {/if}
  {/snippet}
</Modal>

<style>
  .import-body {
    padding: var(--spacing-lg);
    display: flex;
    flex-direction: column;
    gap: var(--spacing-md);
  }

  .import-demo-warning {
    display: flex;
    align-items: flex-start;
    gap: var(--spacing-md);
    padding: var(--spacing-md);
    background: rgba(251, 191, 36, 0.15);
    border: 1px solid rgba(251, 191, 36, 0.4);
    border-radius: 6px;
    color: var(--text-primary);
  }

  .warning-icon {
    color: rgb(251, 191, 36);
    flex-shrink: 0;
    margin-top: 2px;
  }

  .warning-content strong {
    display: block;
    font-size: 13px;
    margin-bottom: 4px;
  }

  .warning-content p {
    margin: 0;
    font-size: 12px;
    color: var(--text-muted);
  }

  .import-error {
    padding: var(--spacing-md);
    background: rgba(239, 68, 68, 0.15);
    border: 1px solid rgba(239, 68, 68, 0.4);
    border-radius: 6px;
    font-size: 13px;
  }

  .import-error .error-message {
    color: var(--accent-danger, #ef4444);
    font-weight: 500;
  }

  .import-error .error-guidance {
    color: var(--text-secondary);
    font-size: 12px;
    margin-top: var(--spacing-xs);
  }

  .import-step {
    display: flex;
    flex-direction: column;
    gap: var(--spacing-md);
  }

  .step-title {
    margin: 0;
    font-size: 14px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .step-hint {
    margin: 0;
    font-size: 13px;
    color: var(--text-muted);
  }

  .preselected-file {
    display: flex;
    align-items: center;
    gap: 6px;
    margin: var(--spacing-xs) 0 0 0;
    font-size: 12px;
    color: var(--text-muted);
  }

  .preselected-file strong {
    color: var(--text-secondary);
    font-weight: 500;
  }

  .loading {
    padding: var(--spacing-lg);
    text-align: center;
    color: var(--text-muted);
    font-size: 13px;
  }

  /* Account list */
  .account-list {
    display: flex;
    flex-direction: column;
    gap: var(--spacing-xs);
    max-height: 250px;
    overflow-y: auto;
  }

  .account-option {
    display: flex;
    flex-direction: row;
    align-items: center;
    justify-content: space-between;
    gap: var(--spacing-sm);
    padding: var(--spacing-sm) var(--spacing-md);
    background: var(--bg-secondary);
    border: 1px solid var(--border-primary);
    border-radius: 6px;
    cursor: pointer;
    transition: all 0.15s;
    text-align: left;
  }

  .account-info {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .account-option:hover {
    border-color: var(--accent-primary);
  }

  .account-option.selected {
    border-color: var(--accent-primary);
    background: rgba(99, 102, 241, 0.1);
  }

  .account-option.create-new {
    flex-direction: row;
    align-items: center;
    gap: var(--spacing-sm);
    color: var(--accent-primary);
    border-style: dashed;
  }

  .account-name {
    font-size: 13px;
    font-weight: 500;
    color: var(--text-primary);
  }

  .account-institution {
    font-size: 11px;
    color: var(--text-muted);
  }

  .account-classification {
    font-size: 10px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    padding: 3px 8px;
    border-radius: 4px;
    background: rgba(34, 197, 94, 0.15);
    color: var(--accent-success, #22c55e);
    flex-shrink: 0;
  }

  .account-classification.liability {
    background: rgba(239, 68, 68, 0.15);
    color: var(--accent-danger, #ef4444);
  }

  /* Create account form wrapper */
  .create-account-form {
    padding: var(--spacing-md);
    background: var(--bg-tertiary);
    border-radius: 8px;
  }

  /* Selected account bar */
  .selected-account-bar {
    display: flex;
    align-items: center;
    gap: var(--spacing-sm);
    padding: var(--spacing-sm) var(--spacing-md);
    background: var(--bg-tertiary);
    border-radius: 4px;
    font-size: 13px;
  }

  .selected-account-bar .label {
    color: var(--text-muted);
  }

  .selected-account-bar .value {
    font-weight: 500;
    color: var(--text-primary);
  }

  /* File selection */
  .file-select-btn {
    padding: var(--spacing-md) var(--spacing-lg);
    background: var(--bg-tertiary);
    border: 2px dashed var(--border-primary);
    border-radius: 8px;
    color: var(--text-primary);
    font-size: 14px;
    cursor: pointer;
    transition: border-color 0.15s, background 0.15s;
  }

  .file-select-btn:hover {
    border-color: var(--accent-primary);
  }

  .btn-link {
    background: none;
    border: none;
    color: var(--accent-primary);
    font-size: 12px;
    cursor: pointer;
    padding: 0;
  }

  .btn-link:hover {
    text-decoration: underline;
  }

  /* Context bar (account + file info) */
  .context-bar {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: var(--spacing-sm) var(--spacing-md);
    background: var(--bg-tertiary);
    border-radius: 6px;
    font-size: 12px;
  }

  .context-item {
    display: flex;
    align-items: center;
    gap: var(--spacing-sm);
  }

  .context-label {
    color: var(--text-muted);
    width: 55px;
    flex-shrink: 0;
  }

  .context-value {
    color: var(--text-primary);
    font-weight: 500;
  }

  .context-value.file-value {
    font-family: var(--font-mono);
    font-weight: 400;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex: 1;
  }

  .context-classification {
    font-size: 10px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    padding: 2px 6px;
    border-radius: 3px;
    background: rgba(34, 197, 94, 0.15);
    color: var(--accent-success, #22c55e);
  }

  .context-classification.liability {
    background: rgba(239, 68, 68, 0.15);
    color: var(--accent-danger, #ef4444);
  }

  /* Preprocessing section (profile + skip rows) */
  .preprocessing-section {
    padding: var(--spacing-sm) var(--spacing-md);
    background: var(--bg-secondary);
    border: 1px solid var(--border-primary);
    border-radius: 6px;
  }

  .preprocessing-row {
    display: flex;
    align-items: center;
    gap: var(--spacing-lg);
  }

  .preprocessing-item {
    display: flex;
    align-items: center;
    gap: var(--spacing-sm);
    font-size: 12px;
  }

  .preprocessing-item label {
    color: var(--text-muted);
    white-space: nowrap;
  }

  .profile-item {
    flex: 1;
  }

  .profile-item select {
    flex: 1;
    min-width: 120px;
    padding: 5px 8px;
    background: var(--bg-primary);
    border: 1px solid var(--border-primary);
    border-radius: 4px;
    color: var(--text-primary);
    font-size: 12px;
    font-family: inherit;
    appearance: none;
    -webkit-appearance: none;
    background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='12' height='12' viewBox='0 0 12 12'%3E%3Cpath fill='%239ca3af' d='M2 4l4 4 4-4'/%3E%3C/svg%3E");
    background-repeat: no-repeat;
    background-position: right 6px center;
    padding-right: 24px;
    cursor: pointer;
  }

  .profile-item select option {
    background: var(--bg-secondary);
    color: var(--text-primary);
    font-family: inherit;
  }

  .skip-item input {
    width: 50px;
    padding: 5px 8px;
    background: var(--bg-primary);
    border: 1px solid var(--border-primary);
    border-radius: 4px;
    color: var(--text-primary);
    font-size: 12px;
    text-align: center;
  }

  .skip-item input:focus,
  .profile-item select:focus {
    outline: none;
    border-color: var(--accent-primary);
  }

  /* Sections */
  .section {
    display: flex;
    flex-direction: column;
    gap: var(--spacing-sm);
  }

  .section-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .section-title {
    font-size: 11px;
    font-weight: 600;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.5px;
  }

  /* Column mapping grid */
  .mapping-grid {
    display: grid;
    grid-template-columns: 90px 1fr;
    gap: var(--spacing-xs) var(--spacing-md);
    align-items: center;
  }

  .mapping-grid label {
    font-size: 12px;
    color: var(--text-secondary);
  }

  .mapping-grid select {
    padding: 7px 10px;
    background: var(--bg-primary);
    border: 1px solid var(--border-primary);
    border-radius: 4px;
    color: var(--text-primary);
    font-size: 12px;
    font-family: inherit;
    appearance: none;
    -webkit-appearance: none;
    background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='12' height='12' viewBox='0 0 12 12'%3E%3Cpath fill='%239ca3af' d='M2 4l4 4 4-4'/%3E%3C/svg%3E");
    background-repeat: no-repeat;
    background-position: right 8px center;
    padding-right: 28px;
    cursor: pointer;
  }

  .mapping-grid select:focus {
    outline: none;
    border-color: var(--accent-primary);
  }

  .mapping-grid select option {
    background: var(--bg-secondary);
    color: var(--text-primary);
    font-family: inherit;
  }

  .optional {
    font-size: 10px;
    color: var(--text-muted);
    font-weight: normal;
  }

  /* Anchor Balance section */
  .anchor-balance-section {
    background: var(--bg-tertiary);
    border-radius: 6px;
    padding: var(--spacing-md);
  }

  .section-hint {
    font-size: 12px;
    color: var(--text-muted);
    margin: var(--spacing-xs) 0 var(--spacing-sm) 0;
  }

  .credit-card-hint {
    font-size: 11px;
    color: var(--text-muted);
    margin: var(--spacing-sm) 0 0 0;
    padding: var(--spacing-xs) var(--spacing-sm);
    background: rgba(251, 191, 36, 0.1);
    border-radius: 4px;
    border-left: 2px solid rgba(251, 191, 36, 0.5);
  }

  .credit-card-hint strong {
    color: var(--text-secondary);
    font-family: var(--font-mono);
  }

  .balance-inputs {
    display: flex;
    gap: var(--spacing-md);
  }

  .balance-input-group {
    display: flex;
    flex-direction: column;
    gap: 4px;
    flex: 1;
  }

  .balance-input-group label {
    font-size: 11px;
    font-weight: 500;
    color: var(--text-muted);
  }

  .input-with-prefix {
    display: flex;
    align-items: center;
    background: var(--bg-primary);
    border: 1px solid var(--border-primary);
    border-radius: 4px;
    overflow: hidden;
  }

  .input-with-prefix:focus-within {
    border-color: var(--accent-primary);
  }

  .input-prefix {
    padding: 6px 10px;
    background: var(--bg-secondary);
    color: var(--text-muted);
    font-size: 12px;
    border-right: 1px solid var(--border-primary);
  }

  .input-with-prefix input {
    flex: 1;
    padding: 6px 10px;
    border: none;
    background: transparent;
    color: var(--text-primary);
    font-size: 12px;
    font-family: var(--font-mono);
    min-width: 0;
  }

  .input-with-prefix input:focus {
    outline: none;
  }

  .balance-input-group input[type="date"] {
    padding: 6px 10px;
    background: var(--bg-primary);
    border: 1px solid var(--border-primary);
    border-radius: 4px;
    color: var(--text-primary);
    font-size: 12px;
  }

  .balance-input-group input[type="date"]:focus {
    outline: none;
    border-color: var(--accent-primary);
  }

  .amount-with-toggle {
    display: flex;
    align-items: center;
    gap: var(--spacing-sm);
  }

  .amount-with-toggle select {
    flex: 1;
  }

  .toggle-link {
    background: none;
    border: none;
    color: var(--text-muted);
    font-size: 11px;
    cursor: pointer;
    padding: 0;
    white-space: nowrap;
  }

  .toggle-link:hover {
    color: var(--accent-primary);
    text-decoration: underline;
  }

  .split-amount-row {
    display: flex;
    align-items: center;
    gap: var(--spacing-sm);
  }

  .split-amount-row select {
    flex: 1;
  }

  /* Format options */
  .format-options {
    display: flex;
    flex-direction: column;
    gap: var(--spacing-sm);
  }

  .format-row {
    display: flex;
    align-items: center;
    gap: var(--spacing-sm);
  }

  .format-row label {
    font-size: 12px;
    color: var(--text-secondary);
  }

  .format-row select {
    padding: 5px 8px;
    background: var(--bg-primary);
    border: 1px solid var(--border-primary);
    border-radius: 4px;
    color: var(--text-primary);
    font-size: 12px;
    font-family: inherit;
    appearance: none;
    -webkit-appearance: none;
    background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='12' height='12' viewBox='0 0 12 12'%3E%3Cpath fill='%239ca3af' d='M2 4l4 4 4-4'/%3E%3C/svg%3E");
    background-repeat: no-repeat;
    background-position: right 6px center;
    padding-right: 24px;
    cursor: pointer;
  }

  .format-row select:focus {
    outline: none;
    border-color: var(--accent-primary);
  }

  .format-row select option {
    background: var(--bg-secondary);
    color: var(--text-primary);
    font-family: inherit;
  }

  .format-checkboxes {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .checkbox-label {
    display: flex;
    align-items: center;
    gap: var(--spacing-sm);
    font-size: 12px;
    color: var(--text-primary);
    cursor: pointer;
  }

  .checkbox-label input {
    margin: 0;
  }

  .checkbox-hint {
    color: var(--text-muted);
  }

  /* Preview */
  .preview-section {
    padding-top: var(--spacing-sm);
    border-top: 1px solid var(--border-primary);
  }

  .preview-loading {
    font-size: 11px;
    color: var(--accent-primary);
    font-style: italic;
  }

  .preview-table {
    border: 1px solid var(--border-primary);
    border-radius: 4px;
    overflow: hidden;
  }

  .preview-body {
    max-height: 250px;
    overflow-y: auto;
  }

  .preview-row {
    display: grid;
    grid-template-columns: 80px 1fr 90px;
    padding: 6px 10px;
    gap: var(--spacing-sm);
    font-size: 12px;
  }

  .preview-table.with-balance .preview-row {
    grid-template-columns: 80px 1fr 90px 90px;
  }

  .preview-row.header {
    background: var(--bg-tertiary);
    font-weight: 600;
    color: var(--text-muted);
  }

  .preview-row:not(.header) {
    border-top: 1px solid var(--border-primary);
  }

  .preview-date {
    color: var(--text-muted);
  }

  .preview-desc {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--text-primary);
  }

  .preview-amount {
    text-align: right;
    font-family: var(--font-mono);
    color: var(--accent-success, #22c55e);
  }

  .preview-amount.negative {
    color: var(--accent-danger, #ef4444);
  }

  .preview-balance {
    text-align: right;
    font-family: var(--font-mono);
    color: var(--text-muted);
  }

  .preview-legend {
    display: flex;
    gap: var(--spacing-md);
    margin-top: var(--spacing-xs);
    font-size: 11px;
  }

  .preview-legend .negative {
    color: var(--accent-danger, #ef4444);
  }

  .preview-legend .positive {
    color: var(--accent-success, #22c55e);
  }

  .preview-legend .preview-count {
    margin-left: auto;
    color: var(--text-muted);
  }

  .preview-empty {
    padding: var(--spacing-md);
    text-align: center;
    color: var(--text-muted);
    font-size: 12px;
    font-style: italic;
    background: var(--bg-secondary);
    border-radius: 4px;
  }

  /* Done state */
  .import-done {
    padding: var(--spacing-md);
  }

  .done-header {
    display: flex;
    align-items: center;
    gap: var(--spacing-md);
    margin-bottom: var(--spacing-lg);
  }

  .done-icon {
    width: 40px;
    height: 40px;
    background: var(--accent-success, #22c55e);
    color: white;
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
  }

  .done-text {
    flex: 1;
  }

  .done-message {
    font-size: 16px;
    font-weight: 600;
    color: var(--text-primary);
    margin: 0;
  }

  .done-skipped {
    font-size: 13px;
    color: var(--text-muted);
    margin: 2px 0 0 0;
  }

  .done-snapshots {
    font-size: 13px;
    color: var(--accent-primary);
    margin: 2px 0 0 0;
  }

  .done-details {
    background: var(--bg-secondary);
    border-radius: 6px;
    padding: var(--spacing-sm) var(--spacing-md);
    margin-bottom: var(--spacing-md);
  }

  .done-detail {
    display: flex;
    justify-content: space-between;
    padding: var(--spacing-xs) 0;
  }

  .done-detail:not(:last-child) {
    border-bottom: 1px solid var(--border-primary);
  }

  .done-label {
    font-size: 12px;
    color: var(--text-muted);
  }

  .done-value {
    font-size: 12px;
    color: var(--text-primary);
    font-weight: 500;
  }

  .undo-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    width: 100%;
    padding: var(--spacing-sm);
    background: transparent;
    border: 1px dashed var(--border-primary);
    border-radius: 6px;
    color: var(--text-muted);
    font-size: 12px;
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .undo-btn:hover:not(:disabled) {
    border-color: var(--accent-danger);
    color: var(--accent-danger);
  }

  .undo-btn:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }

  /* Save profile section */
  .save-profile-section {
    margin-top: var(--spacing-md);
    padding: var(--spacing-md);
    background: var(--bg-tertiary);
    border-radius: 6px;
  }

  .save-profile-header {
    font-size: 12px;
    color: var(--text-muted);
    margin-bottom: var(--spacing-sm);
  }

  .save-profile-input {
    display: flex;
    gap: var(--spacing-sm);
  }

  .save-profile-input input {
    flex: 1;
    padding: 8px 12px;
    background: var(--bg-primary);
    border: 1px solid var(--border-primary);
    border-radius: 4px;
    color: var(--text-primary);
    font-size: 13px;
  }

  .save-profile-input input:focus {
    outline: none;
    border-color: var(--accent-primary);
  }

  .btn-save-profile {
    padding: 8px 16px;
    background: var(--accent-primary);
    border: none;
    border-radius: 4px;
    color: white;
    font-size: 13px;
    font-weight: 500;
    cursor: pointer;
    transition: opacity 0.15s;
  }

  .btn-save-profile:hover:not(:disabled) {
    opacity: 0.9;
  }

  .btn-save-profile:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }

  .account-mapping-checkbox {
    display: flex;
    align-items: center;
    gap: var(--spacing-sm);
    margin-top: var(--spacing-sm);
    font-size: 12px;
    color: var(--text-secondary);
    cursor: pointer;
  }

  .account-mapping-checkbox input {
    margin: 0;
  }
</style>
