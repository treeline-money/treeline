<script lang="ts">
  /**
   * Icon - Simple SVG icon component
   *
   * Usage:
   * <Icon name="settings" />
   * <Icon name="bank" size={20} />
   */

  interface Props {
    name: string;
    size?: number;
    class?: string;
  }

  let { name, size = 16, class: className = "" }: Props = $props();

  // Icon paths (stroke-based, designed for currentColor)
  const icons: Record<string, string> = {
    // Navigation & UI
    "settings": "M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6Z M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1Z",
    "command": "M18 3a3 3 0 0 0-3 3v12a3 3 0 0 0 3 3 3 3 0 0 0 3-3 3 3 0 0 0-3-3H6a3 3 0 0 0-3 3 3 3 0 0 0 3 3 3 3 0 0 0 3-3V6a3 3 0 0 0-3-3 3 3 0 0 0-3 3 3 3 0 0 0 3 3h12a3 3 0 0 0 3-3 3 3 0 0 0-3-3Z",
    "search": "m21 21-4.35-4.35 M11 19a8 8 0 1 0 0-16 8 8 0 0 0 0 16Z",

    // Finance
    "bank": "M3 21h18 M3 10h18 M5 6l7-3 7 3 M4 10v11 M20 10v11 M8 14v3 M12 14v3 M16 14v3",
    "piggy-bank": "M19 5c-1.5 0-2.8 1.4-3 2-3.5-1.5-11-.3-11 5 0 1.8 0 3 2 4.5V20h4v-2h3v2h4v-4c1-.5 1.7-1 2-2h2v-4h-2c0-1-.5-1.5-1-2h0V5Z M2 9v1c0 1.1.9 2 2 2h1 M3 11h-.5a.5.5 0 0 1 0-1H3 M16 11a1 1 0 1 0 0-2 1 1 0 0 0 0 2Z",
    "wallet": "M21 12V7H5a2 2 0 0 1 0-4h14v4 M3 5v14a2 2 0 0 0 2 2h16v-5 M18 12a1 1 0 1 0 0 2 1 1 0 0 0 0-2Z",
    "credit-card": "M1 10h22 M1 6a2 2 0 0 1 2-2h18a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2H3a2 2 0 0 1-2-2V6Z",
    "chart": "M18 20V10 M12 20V4 M6 20v-6",
    "tag": "m20.59 13.41-7.17 7.17a2 2 0 0 1-2.83 0L2 12V2h10l8.59 8.59a2 2 0 0 1 0 2.82Z M7 7h.01",
    "tags": "m15 5 6.3 6.3a2.4 2.4 0 0 1 0 3.4L17 19 M9.586 5.586A2 2 0 0 0 8.172 5H3v5.172a2 2 0 0 0 .586 1.414l8.586 8.586a2 2 0 0 0 2.828 0l5.172-5.172a2 2 0 0 0 0-2.828L11.586 3.586a2 2 0 0 0-1.414-.586H5a2 2 0 0 0-2 2v5.172a2 2 0 0 0 .586 1.414l8.586 8.586 M7 10h.01",
    "target": "M12 22c5.5 0 10-4.5 10-10S17.5 2 12 2 2 6.5 2 12s4.5 10 10 10Z M12 18a6 6 0 1 0 0-12 6 6 0 0 0 0 12Z M12 14a2 2 0 1 0 0-4 2 2 0 0 0 0 4Z",
    "repeat": "m17 2 4 4-4 4 M3 11v-1a4 4 0 0 1 4-4h14 M7 22l-4-4 4-4 M21 13v1a4 4 0 0 1-4 4H3",
    "shield": "M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10Z",

    // Data & Sync
    "database": "M21 5c0 1.1-4 2-9 2S3 6.1 3 5 M21 5c0-1.1-4-2-9-2s-9 .9-9 2 M21 5v14c0 1.1-4 2-9 2s-9-.9-9-2V5 M21 12c0 1.1-4 2-9 2s-9-.9-9-2",
    "refresh": "M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8 M21 3v5h-5 M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16 M8 16H3v5",
    "link": "M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71 M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71",
    "zap": "M13 2 3 14h9l-1 8 10-12h-9l1-8Z",

    // Charts & Flow
    "activity": "M22 12h-2.48a2 2 0 0 0-1.93 1.46l-2.35 8.36a.25.25 0 0 1-.48 0L9.24 2.18a.25.25 0 0 0-.48 0l-2.35 8.36A2 2 0 0 1 4.49 12H2",
    "trending-up": "m22 7-8.5 8.5-5-5L2 17 M16 7h6v6",

    // UI elements
    "palette": "M12 2.69l5.66 5.66a8 8 0 1 1-11.31 0L12 2.69Z M12 2v4.5 M3 12h4.5 M12 21.5V17 M20.5 12H16",
    "info": "M12 22c5.5 0 10-4.5 10-10S17.5 2 12 2 2 6.5 2 12s4.5 10 10 10Z M12 16v-4 M12 8h.01",
    "play": "m5 3 14 9-14 9V3Z",
    "arrow-right": "M5 12h14 M12 5l7 7-7 7",
    "log-out": "M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4 M16 17l5-5-5-5 M21 12H9",
    "beaker": "M4.5 3h15 M6 3v16a2 2 0 0 0 2 2h8a2 2 0 0 0 2-2V3 M6 14h12",
    "check": "M20 6 9 17l-5-5",
    "copy": "M20 9h-9a2 2 0 0 0-2 2v9a2 2 0 0 0 2 2h9a2 2 0 0 0 2-2v-9a2 2 0 0 0-2-2Z M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1",
    "x": "M18 6 6 18 M6 6l12 12",
    "chevron-down": "m6 9 6 6 6-6",
    "chevron-right": "m9 18 6-6-6-6",
    "plus": "M12 5v14 M5 12h14",
    "minus": "M5 12h14",
    "edit": "M17 3a2.85 2.83 0 1 1 4 4L7.5 20.5 2 22l1.5-5.5Z",
    "trash": "M3 6h18 M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6 M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2",
    "eye": "M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8Z M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6Z",
    "external-link": "M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6 M15 3h6v6 M10 14 21 3",
    "pin": "M12 17v5 M9 10.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24V16a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1v-.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.76V7a1 1 0 0 1 1-1 2 2 0 0 0 0-4H8a2 2 0 0 0 0 4 1 1 0 0 1 1 1z",

    // Files & Packages
    "package": "m7.5 4.27 9 5.15 M21 8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16Z M3.29 7 12 12l8.71-5 M12 22V12",
    "file": "M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8Z M14 2v6h6",
    "file-text": "M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8Z M14 2v6h6 M16 13H8 M16 17H8 M10 9H8",
    "download": "M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4 M7 10l5 5 5-5 M12 15V3",

    // Status & Alerts
    "check-circle": "M22 11.08V12a10 10 0 1 1-5.93-9.14 M22 4 12 14.01l-3-3",
    "alert-triangle": "m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3Z M12 9v4 M12 17h.01",
    "arrow-up-circle": "M12 22c5.5 0 10-4.5 10-10S17.5 2 12 2 2 6.5 2 12s4.5 10 10 10Z M16 12l-4-4-4 4 M12 16V8",

    // Security
    "lock": "M19 11H5a2 2 0 0 0-2 2v7a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7a2 2 0 0 0-2-2Z M7 11V7a5 5 0 0 1 10 0v4",
    "unlock": "M19 11H5a2 2 0 0 0-2 2v7a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7a2 2 0 0 0-2-2Z M7 11V7a5 5 0 0 1 9.9-1",
  };

  let path = $derived(icons[name] || "");
</script>

{#if path}
  <svg
    xmlns="http://www.w3.org/2000/svg"
    width={size}
    height={size}
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    stroke-width="2"
    stroke-linecap="round"
    stroke-linejoin="round"
    class="icon {className}"
    aria-hidden="true"
  >
    <path d={path} />
  </svg>
{:else}
  <!-- Fallback for unknown icons -->
  <span class="icon-fallback" style="width: {size}px; height: {size}px;">?</span>
{/if}

<style>
  .icon {
    display: inline-block;
    vertical-align: middle;
    flex-shrink: 0;
  }

  .icon-fallback {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    font-size: 10px;
    color: var(--text-muted);
  }
</style>
