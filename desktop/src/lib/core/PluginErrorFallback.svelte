<script lang="ts">
  interface Props {
    error: Error;
    pluginName?: string;
    reset?: () => void;
  }

  let { error, pluginName, reset }: Props = $props();

  let showDetails = $state(false);
</script>

<div class="plugin-error">
  <div class="error-icon">!</div>
  <div class="error-content">
    <h3 class="error-title">
      {pluginName ? `${pluginName} crashed` : 'Plugin crashed'}
    </h3>
    <p class="error-message">{error.message}</p>
    <div class="error-actions">
      {#if reset}
        <button class="retry-btn" onclick={reset}>Retry</button>
      {/if}
      <button class="details-btn" onclick={() => showDetails = !showDetails}>
        {showDetails ? 'Hide details' : 'Details'}
      </button>
    </div>
    {#if showDetails && error.stack}
      <pre class="error-stack">{error.stack}</pre>
    {/if}
  </div>
</div>

<style>
  .plugin-error {
    display: flex;
    align-items: flex-start;
    gap: var(--spacing-md);
    padding: var(--spacing-xl);
    margin: var(--spacing-lg);
    background: var(--bg-secondary);
    border: 1px solid var(--border-primary);
    border-radius: var(--radius-md);
    max-width: 600px;
    margin-left: auto;
    margin-right: auto;
    margin-top: 10vh;
  }

  .error-icon {
    flex-shrink: 0;
    width: 32px;
    height: 32px;
    border-radius: 50%;
    background: color-mix(in srgb, var(--accent-danger) 15%, transparent);
    color: var(--accent-danger);
    display: flex;
    align-items: center;
    justify-content: center;
    font-weight: 700;
    font-size: 16px;
  }

  .error-content {
    flex: 1;
    min-width: 0;
  }

  .error-title {
    margin: 0 0 var(--spacing-xs);
    font-size: 14px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .error-message {
    margin: 0 0 var(--spacing-md);
    font-size: 13px;
    color: var(--text-secondary);
    line-height: 1.4;
  }

  .error-actions {
    display: flex;
    gap: var(--spacing-sm);
  }

  .retry-btn,
  .details-btn {
    padding: 4px 12px;
    border-radius: var(--radius-sm);
    font-size: 12px;
    font-family: var(--font-sans);
    cursor: pointer;
    border: 1px solid var(--border-primary);
    transition: background 0.1s;
  }

  .retry-btn {
    background: var(--accent-primary);
    color: white;
    border-color: var(--accent-primary);
  }

  .retry-btn:hover {
    opacity: 0.9;
  }

  .details-btn {
    background: var(--bg-tertiary);
    color: var(--text-secondary);
  }

  .details-btn:hover {
    background: var(--bg-hover);
  }

  .error-stack {
    margin: var(--spacing-md) 0 0;
    padding: var(--spacing-sm);
    background: var(--bg-tertiary);
    border-radius: var(--radius-sm);
    font-family: var(--font-mono);
    font-size: 11px;
    color: var(--text-secondary);
    overflow-x: auto;
    white-space: pre-wrap;
    word-break: break-all;
    max-height: 200px;
    overflow-y: auto;
  }
</style>
