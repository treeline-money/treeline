<script lang="ts">
  import { activityStore } from "../sdk/activity.svelte";

  // Access activities array directly for proper Svelte 5 reactivity tracking
  let activities = $derived(activityStore.activities);
  let currentActivity = $derived(activities[0] ?? null);
  let activityCount = $derived(activities.length);
</script>

{#if currentActivity}
  <div class="statusbar-activity">
    <span class="activity-spinner"></span>
    <span class="activity-label">{currentActivity.label}</span>
    {#if activityCount > 1}
      <span class="activity-count">+{activityCount - 1}</span>
    {/if}
  </div>
{/if}

<style>
  .statusbar-activity {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 2px 8px;
    color: var(--text-secondary);
    font-size: 11px;
  }

  .activity-spinner {
    width: 10px;
    height: 10px;
    border: 1.5px solid var(--text-muted);
    border-top-color: var(--accent-primary);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  .activity-label {
    color: var(--text-secondary);
  }

  .activity-count {
    color: var(--text-muted);
    font-size: 10px;
  }
</style>
