<script lang="ts">
  /**
   * PendingImportsModal - Shows pending files in the watch folder on startup
   * User can continue to import or dismiss until next app launch
   */
  import { Modal, Icon } from "../shared";
  import type { PendingImportFile } from "../sdk";

  interface Props {
    open: boolean;
    files: PendingImportFile[];
    onclose: () => void;
    oncontinue: (filePath: string) => void;
  }

  let { open, files, onclose, oncontinue }: Props = $props();

  function formatFileSize(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  }

  function handleContinue() {
    if (files.length > 0) {
      oncontinue(files[0].path);
    }
  }
</script>

<Modal {open} title="Files Ready to Import" onclose={onclose} width="480px">
  <div class="content">
    <p class="description">
      {files.length === 1
        ? "A CSV file is waiting in your import folder."
        : `${files.length} CSV files are waiting in your import folder.`}
    </p>

    <div class="file-list">
      {#each files as file}
        <div class="file-item">
          <Icon name="file-text" size={16} />
          <span class="filename">{file.filename}</span>
          <span class="filesize">{formatFileSize(file.size_bytes)}</span>
        </div>
      {/each}
    </div>

    <p class="hint">
      {files.length === 1
        ? "Continue to configure column mappings and import."
        : "You'll be able to import each file one at a time."}
    </p>
  </div>

  {#snippet actions()}
    <button class="btn secondary" onclick={onclose}>Remind Me Later</button>
    <button class="btn primary" onclick={handleContinue}>
      Continue
      <Icon name="arrow-right" size={14} />
    </button>
  {/snippet}
</Modal>

<style>
  .content {
    padding: var(--spacing-md) var(--spacing-lg);
  }

  .description {
    margin: 0 0 var(--spacing-md) 0;
    font-size: 14px;
    color: var(--text-secondary);
  }

  .file-list {
    display: flex;
    flex-direction: column;
    gap: var(--spacing-xs);
    max-height: 200px;
    overflow-y: auto;
    margin-bottom: var(--spacing-md);
  }

  .file-item {
    display: flex;
    align-items: center;
    gap: var(--spacing-sm);
    padding: var(--spacing-sm) var(--spacing-md);
    background: var(--bg-tertiary);
    border-radius: 6px;
    font-size: 13px;
  }

  .file-item :global(svg) {
    color: var(--text-muted);
    flex-shrink: 0;
  }

  .filename {
    flex: 1;
    color: var(--text-primary);
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .filesize {
    color: var(--text-muted);
    font-size: 12px;
    flex-shrink: 0;
  }

  .hint {
    margin: 0;
    font-size: 12px;
    color: var(--text-muted);
  }

  .btn.primary {
    display: flex;
    align-items: center;
    gap: var(--spacing-xs);
  }

  .btn.primary :global(svg) {
    color: inherit;
  }
</style>
