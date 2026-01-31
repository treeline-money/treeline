<script lang="ts">
  import { onMount } from "svelte";
  import { getVersion } from "@tauri-apps/api/app";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { Modal } from "../shared";
  import { getAppSetting, setAppSetting } from "../sdk/settings";

  interface Props {
    onclose: () => void;
  }

  let { onclose }: Props = $props();

  let appVersion = $state<string>("");
  let releaseNotes = $state<string>("");
  let releaseUrl = $state<string>("");
  let isLoading = $state(true);
  let error = $state<string | null>(null);

  onMount(async () => {
    try {
      appVersion = await getVersion();
      await fetchReleaseNotes(appVersion);
    } catch (e) {
      console.error("Failed to load release notes:", e);
      error = "Failed to load release notes";
    } finally {
      isLoading = false;
    }
  });

  async function fetchReleaseNotes(version: string) {
    try {
      // Fetch release notes from GitHub API
      const response = await fetch(
        `https://api.github.com/repos/treeline-money/treeline/releases/tags/${version}`
      );

      if (response.ok) {
        const data = await response.json();
        releaseUrl = data.html_url || "";
        let notes = data.body || "";

        // Clean up GitHub's auto-generated "Full Changelog" link
        notes = notes.replace(/\*\*Full Changelog\*\*:.*$/gm, "").trim();

        if (!notes) {
          notes = "Bug fixes and improvements.";
        }

        releaseNotes = notes;
      } else if (response.status === 404) {
        // Version might not have a release yet (dev build)
        releaseNotes = "You're running a development build.";
      } else {
        throw new Error(`HTTP ${response.status}`);
      }
    } catch (e) {
      console.error("Failed to fetch release notes:", e);
      releaseNotes = "Unable to fetch release notes.";
    }
  }

  async function handleClose() {
    // Mark this version as seen
    await setAppSetting("lastSeenVersion", appVersion);
    onclose();
  }

  function handleViewOnGitHub() {
    if (releaseUrl) {
      openUrl(releaseUrl);
    }
  }

  // Simple markdown-like rendering (headers and bullets)
  function renderNotes(notes: string): string {
    return (
      notes
        // Convert ## headers to styled divs
        .replace(/^## (.+)$/gm, '<h2 class="notes-title">$1</h2>')
        // Convert ### headers to styled divs
        .replace(/^### (.+)$/gm, '<h3 class="notes-section">$1</h3>')
        // Convert bullet points
        .replace(/^- (.+)$/gm, "<li>$1</li>")
        // Wrap consecutive li elements in ul (remove newlines inside ul)
        .replace(/(<li>.*<\/li>\n?)+/g, (match) => {
          return '<ul class="notes-list">' + match.replace(/\n/g, "") + "</ul>";
        })
        // Convert **bold** to strong
        .replace(/\*\*(.+?)\*\*/g, "<strong>$1</strong>")
        // Convert URLs to links
        .replace(
          /(https?:\/\/[^\s<]+)/g,
          '<a href="$1" class="notes-link">$1</a>'
        )
        // Convert double newlines to paragraph breaks
        .replace(/\n\n+/g, "</p><p>")
        // Remove single newlines (they're just line wrapping in markdown)
        .replace(/\n/g, " ")
    );
  }
</script>

<Modal open={true} title="What's New" onclose={handleClose} width="520px">
  <div class="whats-new-content">
    {#if isLoading}
      <div class="loading">
        <span class="spinner"></span>
        <span>Loading release notes...</span>
      </div>
    {:else if error}
      <div class="error">{error}</div>
    {:else}
      <div class="version-header">
        <span class="version-badge">v{appVersion}</span>
        {#if releaseUrl}
          <button class="link-btn" onclick={handleViewOnGitHub}>
            View on GitHub
          </button>
        {/if}
      </div>
      <div class="release-notes">
        {@html renderNotes(releaseNotes)}
      </div>
    {/if}
  </div>

  {#snippet actions()}
    <button class="btn primary" onclick={handleClose}>Got it!</button>
  {/snippet}
</Modal>

<style>
  .whats-new-content {
    min-height: 120px;
    padding: var(--spacing-md) var(--spacing-lg);
  }

  .loading {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--spacing-md);
    padding: var(--spacing-xl);
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
    to {
      transform: rotate(360deg);
    }
  }

  .error {
    padding: var(--spacing-lg);
    color: var(--accent-danger);
    text-align: center;
  }

  .version-header {
    display: flex;
    align-items: center;
    gap: var(--spacing-md);
    margin-bottom: var(--spacing-lg);
  }

  .version-badge {
    display: inline-block;
    padding: 6px 14px;
    background: var(--accent-primary);
    color: white;
    font-size: 13px;
    font-weight: 600;
    border-radius: 16px;
  }

  .link-btn {
    background: none;
    border: none;
    color: var(--text-muted);
    font-size: 12px;
    cursor: pointer;
    text-decoration: underline;
    padding: 0;
  }

  .link-btn:hover {
    color: var(--accent-primary);
  }

  .release-notes {
    font-size: 14px;
    line-height: 1.7;
    color: var(--text-primary);
  }

  .release-notes :global(p) {
    margin: 0;
  }

  .release-notes :global(h2.notes-title) {
    font-size: 16px;
    font-weight: 600;
    margin: 0 0 var(--spacing-md) 0;
    color: var(--text-primary);
  }

  .release-notes :global(h3.notes-section) {
    font-size: 12px;
    font-weight: 600;
    margin: var(--spacing-lg) 0 var(--spacing-sm) 0;
    color: var(--accent-primary);
    text-transform: uppercase;
    letter-spacing: 0.5px;
  }

  .release-notes :global(h3.notes-section:first-child) {
    margin-top: 0;
  }

  .release-notes :global(ul.notes-list) {
    margin: 0 0 var(--spacing-sm) 0;
    padding-left: var(--spacing-lg);
  }

  .release-notes :global(li) {
    margin-bottom: var(--spacing-xs);
    color: var(--text-primary);
  }

  .release-notes :global(a.notes-link) {
    color: var(--accent-primary);
    text-decoration: none;
    word-break: break-all;
  }

  .release-notes :global(a.notes-link:hover) {
    text-decoration: underline;
  }

  .btn {
    padding: 8px 20px;
    border-radius: 6px;
    font-size: 13px;
    font-weight: 500;
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .btn.primary {
    background: var(--accent-primary);
    border: none;
    color: white;
  }

  .btn.primary:hover {
    filter: brightness(1.1);
  }
</style>
