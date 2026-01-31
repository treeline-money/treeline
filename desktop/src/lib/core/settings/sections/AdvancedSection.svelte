<script lang="ts">
  import { Icon } from "../../../shared";
  import "../settings-shared.css";

  interface Props {
    developerMode: boolean;
    onDeveloperModeChange: (enabled: boolean) => void;
  }

  let { developerMode, onDeveloperModeChange }: Props = $props();
</script>

<section class="section">
  <h3 class="section-title">Advanced</h3>

  <div class="setting-group">
    <h4 class="group-title">Developer Mode</h4>
    <p class="group-desc">
      Enable browser DevTools for inspecting and debugging plugin UI.
      Useful for plugin developers. You can also toggle with
      <kbd>Cmd</kbd>+<kbd>Shift</kbd>+<kbd>I</kbd> (Mac) or
      <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>I</kbd> (Windows/Linux).
    </p>

    <label class="checkbox-setting">
      <input
        type="checkbox"
        checked={developerMode}
        onchange={(e) => onDeveloperModeChange(e.currentTarget.checked)}
      />
      <span>Enable Developer Mode</span>
    </label>

    {#if developerMode}
      <div class="dev-mode-active">
        <Icon name="check-circle" size={14} />
        <span>DevTools are available. Right-click and select "Inspect" or use the keyboard shortcut.</span>
      </div>
    {/if}
  </div>
</section>

<style>
  kbd {
    display: inline-block;
    padding: 2px 6px;
    font-size: 11px;
    font-family: var(--font-mono);
    background: var(--bg-tertiary);
    border: 1px solid var(--border-primary);
    border-radius: 4px;
    color: var(--text-secondary);
  }

  .dev-mode-active {
    display: flex;
    align-items: center;
    gap: var(--spacing-sm);
    padding: var(--spacing-sm) var(--spacing-md);
    background: rgba(34, 197, 94, 0.1);
    border: 1px solid rgba(34, 197, 94, 0.3);
    border-radius: 6px;
    font-size: 12px;
    color: var(--text-secondary);
  }

  .dev-mode-active :global(svg) {
    color: #22c55e;
    flex-shrink: 0;
  }
</style>
