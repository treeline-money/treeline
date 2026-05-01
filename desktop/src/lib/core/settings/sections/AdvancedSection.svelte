<script lang="ts">
  import { Icon } from "../../../shared";
  import { featureFlags, KNOWN_FLAGS } from "../../../sdk";
  import "../settings-shared.css";

  interface Props {
    developerMode: boolean;
    pluginHotReload: boolean;
    onDeveloperModeChange: (enabled: boolean) => void;
    onPluginHotReloadChange: (enabled: boolean) => void;
  }

  let { developerMode, pluginHotReload, onDeveloperModeChange, onPluginHotReloadChange }: Props = $props();

  async function toggleFlag(name: string, enabled: boolean) {
    await featureFlags.set(name, enabled);
  }
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

      <div class="sub-setting">
        <h4 class="group-title">Plugin Hot-Reload</h4>
        <p class="group-desc">
          Automatically reload external plugins when their files change.
          Watches <code>~/.treeline/plugins/</code> for changes to <code>index.js</code> and <code>manifest.json</code>.
        </p>

        <label class="checkbox-setting">
          <input
            type="checkbox"
            checked={pluginHotReload}
            onchange={(e) => onPluginHotReloadChange(e.currentTarget.checked)}
          />
          <span>Enable Plugin Hot-Reload</span>
        </label>

        {#if pluginHotReload}
          <div class="dev-mode-active">
            <Icon name="check-circle" size={14} />
            <span>Watching for plugin file changes. Plugins will reload automatically on save.</span>
          </div>
        {/if}
      </div>
    {/if}
  </div>

  <div class="setting-group">
    <h4 class="group-title">Experimental features</h4>
    <p class="group-desc">
      Toggle in-progress features. These may change or break between releases.
    </p>

    {#each KNOWN_FLAGS as flag (flag.name)}
      <label class="checkbox-setting">
        <input
          type="checkbox"
          checked={featureFlags.isEnabled(flag.name)}
          onchange={(e) => toggleFlag(flag.name, e.currentTarget.checked)}
        />
        <div class="flag-text">
          <span class="flag-label">{flag.label}</span>
          <span class="flag-desc">{flag.description}</span>
        </div>
      </label>
    {/each}
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

  .sub-setting {
    margin-top: var(--spacing-md);
    padding-top: var(--spacing-md);
    border-top: 1px solid var(--border-primary);
  }

  code {
    font-family: var(--font-mono);
    font-size: 12px;
    padding: 1px 4px;
    background: var(--bg-tertiary);
    border-radius: 3px;
    color: var(--text-secondary);
  }

  .flag-text {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .flag-label {
    color: var(--text-primary);
    font-size: 13px;
    font-weight: 500;
  }
  .flag-desc {
    color: var(--text-muted);
    font-size: 12px;
  }
</style>
