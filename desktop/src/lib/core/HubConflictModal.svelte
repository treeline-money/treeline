<script lang="ts">
  import { Icon } from "../shared";
  import {
    getHubConflicts,
    resolveHubConflicts,
    registry,
    toast,
    type ConflictReport,
  } from "../sdk";
  import "./settings/settings-shared.css";

  interface Props {
    isOpen: boolean;
    onClose: () => void;
  }

  let { isOpen, onClose }: Props = $props();

  type Phase = "loading" | "ready" | "resolving" | "error";
  let phase = $state<Phase>("loading");
  let report = $state<ConflictReport | null>(null);
  let errorMessage = $state<string | null>(null);
  let resolvingSide = $state<"local" | "hub" | null>(null);

  $effect(() => {
    if (isOpen) {
      void load();
    } else {
      phase = "loading";
      report = null;
      errorMessage = null;
      resolvingSide = null;
    }
  });

  async function load() {
    phase = "loading";
    errorMessage = null;
    try {
      report = await getHubConflicts();
      phase = "ready";
    } catch (e) {
      errorMessage = String(e);
      phase = "error";
    }
  }

  async function resolve(keep: "local" | "hub") {
    phase = "resolving";
    resolvingSide = keep;
    try {
      const result = await resolveHubConflicts(keep);
      if (result.status === "no_base_snapshot") {
        errorMessage =
          "No merge base available — resolve from the CLI with 'tl hub push --force' or 'tl hub pull'.";
        phase = "error";
        return;
      }
      toast.success(
        keep === "local"
          ? "Conflict resolved — kept this device's values"
          : "Conflict resolved — kept the hub's values",
      );
      registry.emit("data:refresh");
      onClose();
    } catch (e) {
      errorMessage = String(e);
      phase = "error";
    }
  }

  function fmt(value: unknown): string {
    const s =
      value === null || value === undefined
        ? "(empty)"
        : typeof value === "string"
          ? value
          : JSON.stringify(value);
    return s.length > 80 ? s.slice(0, 77) + "…" : s;
  }

  function fmtRow(row: Record<string, unknown>): string {
    const s = Object.entries(row)
      .map(([k, v]) => `${k}: ${fmt(v)}`)
      .join(", ");
    return s.length > 120 ? s.slice(0, 117) + "…" : s;
  }

  let otherChanges = $derived(
    report ? report.local_only_changes + report.hub_only_changes : 0,
  );
</script>

{#if isOpen}
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div
    class="sub-modal-overlay"
    onclick={onClose}
    onkeydown={(e) => e.key === "Escape" && onClose()}
    role="dialog"
    aria-modal="true"
    tabindex="-1"
  >
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="sub-modal conflict-modal" onclick={(e) => e.stopPropagation()}>
      <div class="sub-modal-header">
        <span class="sub-modal-title">Resolve sync conflict</span>
        <button class="close-btn" onclick={onClose} aria-label="Close">
          <Icon name="x" size={18} />
        </button>
      </div>

      <div class="sub-modal-body">
        {#if phase === "loading"}
          <div class="loading">Comparing this device with the hub…</div>
        {:else if phase === "error"}
          <p class="group-desc conflict-error">{errorMessage}</p>
          <div class="sub-modal-actions">
            <button class="btn secondary" onclick={() => void load()}>Retry</button>
            <button class="btn secondary" onclick={onClose}>Close</button>
          </div>
        {:else if report && report.total_conflicts === 0}
          <p class="group-desc">
            No conflicts found — this device and the hub merge cleanly. The
            watcher will sync automatically.
          </p>
          <div class="sub-modal-actions">
            <button class="btn secondary" onclick={onClose}>Close</button>
          </div>
        {:else if report}
          <p class="group-desc">
            This device and the hub changed the same
            {report.total_conflicts === 1 ? "value" : "values"} in different
            ways. Pick which side wins — everything else
            {#if otherChanges > 0}({otherChanges} non-conflicting change{otherChanges === 1 ? "" : "s"}){/if}
            is kept from both sides either way.
          </p>

          <div class="conflict-list">
            {#each report.tables as table (table.table)}
              <div class="conflict-table">
                <div class="conflict-table-name">{table.table}</div>
                {#each table.conflicts as conflict}
                  {#if conflict.kind === "modified"}
                    <div class="conflict-row">
                      <div class="conflict-row-key" title={fmtRow(conflict.key)}>
                        {fmtRow(conflict.key)}
                      </div>
                      {#each conflict.columns as col (col.column)}
                        <div class="conflict-values">
                          <span class="conflict-col">{col.column}</span>
                          <span class="conflict-side">
                            <span class="side-label">this device</span>
                            {fmt(col.local)}
                          </span>
                          <span class="conflict-side">
                            <span class="side-label">hub</span>
                            {fmt(col.hub)}
                          </span>
                        </div>
                      {/each}
                    </div>
                  {:else if conflict.kind === "both_added"}
                    <div class="conflict-row">
                      <div class="conflict-row-key">Both sides added the same row</div>
                      <div class="conflict-values">
                        <span class="conflict-side">
                          <span class="side-label">this device</span>
                          {fmtRow(conflict.local_row)}
                        </span>
                        <span class="conflict-side">
                          <span class="side-label">hub</span>
                          {fmtRow(conflict.hub_row)}
                        </span>
                      </div>
                    </div>
                  {:else}
                    <div class="conflict-row">
                      <div class="conflict-row-key">
                        Deleted on one side, modified on the other
                      </div>
                      <div class="conflict-values">
                        <span class="conflict-side">
                          <span class="side-label">deleted</span>
                          {fmtRow(conflict.deleted_row)}
                        </span>
                        <span class="conflict-side">
                          <span class="side-label">modified</span>
                          {fmtRow(conflict.modified_row)}
                        </span>
                      </div>
                    </div>
                  {/if}
                {/each}
              </div>
            {/each}
          </div>

          <p class="group-desc conflict-note">
            The hub keeps backups of recent versions, so nothing is
            unrecoverable.
          </p>

          <div class="sub-modal-actions">
            <button class="btn secondary" onclick={onClose}>Cancel</button>
            <button class="btn" onclick={() => void resolve("hub")}>
              Keep hub's values
            </button>
            <button class="btn" onclick={() => void resolve("local")}>
              Keep this device's values
            </button>
          </div>
        {/if}

        {#if phase === "resolving"}
          <div class="loading">
            Merging and pushing ({resolvingSide === "local"
              ? "keeping this device's values"
              : "keeping the hub's values"})…
          </div>
        {/if}
      </div>
    </div>
  </div>
{/if}

<style>
  .conflict-modal {
    max-width: 640px;
    width: 90vw;
  }
  .conflict-error {
    color: var(--color-danger, #dc2626);
  }
  .conflict-list {
    max-height: 40vh;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 12px;
    margin: 12px 0;
  }
  .conflict-table-name {
    font-weight: 600;
    font-size: 13px;
    margin-bottom: 4px;
  }
  .conflict-row {
    padding: 8px 10px;
    border: 1px solid var(--border-color, #333);
    border-radius: 6px;
    margin-bottom: 6px;
    font-size: 12px;
  }
  .conflict-row-key {
    color: var(--text-secondary, #999);
    margin-bottom: 6px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .conflict-values {
    display: flex;
    flex-direction: column;
    gap: 4px;
    margin-bottom: 4px;
  }
  .conflict-col {
    font-weight: 600;
  }
  .conflict-side {
    display: flex;
    gap: 8px;
    align-items: baseline;
    word-break: break-word;
  }
  .side-label {
    flex: none;
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--text-secondary, #999);
    min-width: 72px;
  }
</style>
