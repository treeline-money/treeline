/**
 * Feature flags - lightweight UI gating for in-progress features.
 *
 * Stored in `settings.json` under `app.experimentalFeatures: { [name]: boolean }`.
 * Reads/writes go through the existing settings infrastructure, so flags
 * survive across launches and are visible to anyone inspecting `settings.json`.
 *
 * Scope: flags gate **UI surface**, not the underlying functionality. A user
 * can disable a flag and still have the feature working in the background
 * (e.g. hub watcher keeps running). The flag is purely about whether we
 * advertise the feature in the UI.
 *
 * To add a flag:
 *   1. Add a `FEATURE_*` constant below.
 *   2. Add it to `KNOWN_FLAGS` so the Advanced settings panel renders it.
 *   3. Gate the UI surface with `featureFlags.isEnabled(FEATURE_*)`.
 */

import { getAppSetting, setAppSetting } from "./settings";

/** Hub linking, push/pull, in-process watcher status badge. */
export const FEATURE_HUB = "hub";

export interface KnownFlag {
  name: string;
  label: string;
  description: string;
}

/**
 * Flags rendered in Settings → Advanced → Experimental features.
 * Order in this list is the display order.
 */
export const KNOWN_FLAGS: KnownFlag[] = [
  {
    name: FEATURE_HUB,
    label: "Hub",
    description:
      "Link this device to a Treeline hub for cross-device data flow. Surfaces a Hub settings panel and a status badge in the bottom bar.",
  },
];

class FeatureFlagStore {
  private _flags = $state<Record<string, boolean>>({});
  private _loaded = $state(false);

  /** Whether `load()` has completed at least once. UI can wait on this to
   *  avoid flickering features in/out during startup. */
  get loaded() {
    return this._loaded;
  }

  /** Returns `true` if the named flag is enabled. Unknown flags are off. */
  isEnabled(name: string): boolean {
    return !!this._flags[name];
  }

  /** Snapshot of all current flag values (read-only). */
  get all(): Readonly<Record<string, boolean>> {
    return this._flags;
  }

  /** Load flags from `settings.json`. Safe to call multiple times. */
  async load(): Promise<void> {
    try {
      const value = (await getAppSetting("experimentalFeatures")) as
        | Record<string, boolean>
        | undefined;
      this._flags = value ?? {};
    } catch (e) {
      console.warn("[feature-flags] load failed:", e);
      this._flags = {};
    } finally {
      this._loaded = true;
    }
  }

  /** Toggle a flag on/off. Persists to settings and updates the in-memory
   *  state synchronously (after the await) so the UI reacts immediately. */
  async set(name: string, enabled: boolean): Promise<void> {
    const next = { ...this._flags, [name]: enabled };
    // Strip falsy entries to keep settings.json tidy.
    const cleaned: Record<string, boolean> = {};
    for (const [k, v] of Object.entries(next)) {
      if (v) cleaned[k] = true;
    }
    await setAppSetting("experimentalFeatures", cleaned);
    this._flags = next;
  }
}

export const featureFlags = new FeatureFlagStore();
