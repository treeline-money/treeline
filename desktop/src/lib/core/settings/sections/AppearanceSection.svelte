<script lang="ts">
  import type { AppSettings } from "../../../sdk";
  import "../settings-shared.css";

  interface Props {
    currentTheme: AppSettings["theme"];
    onThemeChange: (theme: AppSettings["theme"]) => void;
  }

  let { currentTheme, onThemeChange }: Props = $props();
</script>

<section class="section">
  <h3 class="section-title">Appearance</h3>

  <div class="setting-group">
    <h4 class="group-title">Theme</h4>

    <div class="theme-options">
      <label class="theme-option" class:selected={currentTheme === "light"}>
        <input
          type="radio"
          name="theme"
          value="light"
          checked={currentTheme === "light"}
          onchange={() => onThemeChange("light")}
        />
        <div class="theme-preview light">
          <div class="preview-bar"></div>
          <div class="preview-content"></div>
        </div>
        <span class="theme-label">Light</span>
      </label>

      <label class="theme-option" class:selected={currentTheme === "dark"}>
        <input
          type="radio"
          name="theme"
          value="dark"
          checked={currentTheme === "dark"}
          onchange={() => onThemeChange("dark")}
        />
        <div class="theme-preview dark">
          <div class="preview-bar"></div>
          <div class="preview-content"></div>
        </div>
        <span class="theme-label">Dark</span>
      </label>

      <label class="theme-option" class:selected={currentTheme === "system"}>
        <input
          type="radio"
          name="theme"
          value="system"
          checked={currentTheme === "system"}
          onchange={() => onThemeChange("system")}
        />
        <div class="theme-preview system">
          <div class="preview-bar"></div>
          <div class="preview-content"></div>
        </div>
        <span class="theme-label">System</span>
      </label>
    </div>
  </div>
</section>

<style>
  .theme-options {
    display: flex;
    gap: var(--spacing-md);
  }

  .theme-option {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--spacing-sm);
    cursor: pointer;
  }

  .theme-option input {
    display: none;
  }

  .theme-preview {
    width: 72px;
    height: 48px;
    border-radius: 6px;
    border: 2px solid var(--border-primary);
    overflow: hidden;
    transition: border-color 0.15s;
  }

  .theme-option.selected .theme-preview {
    border-color: var(--accent-primary);
  }

  .theme-preview.light {
    background: #f5f5f5;
  }

  .theme-preview.light .preview-bar {
    height: 10px;
    background: #e0e0e0;
  }

  .theme-preview.dark {
    background: #1a1a1a;
  }

  .theme-preview.dark .preview-bar {
    height: 10px;
    background: #2a2a2a;
  }

  .theme-preview.system {
    background: linear-gradient(90deg, #f5f5f5 50%, #1a1a1a 50%);
  }

  .theme-preview.system .preview-bar {
    height: 10px;
    background: linear-gradient(90deg, #e0e0e0 50%, #2a2a2a 50%);
  }

  .theme-label {
    font-size: 12px;
    color: var(--text-secondary);
  }

  .theme-option.selected .theme-label {
    color: var(--text-primary);
    font-weight: 500;
  }
</style>
