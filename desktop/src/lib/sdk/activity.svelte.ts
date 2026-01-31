/**
 * Activity Store - Track active operations for status bar display
 */

export interface Activity {
  id: string;
  label: string;
  startedAt: number;
}

class ActivityStore {
  private _activities = $state<Activity[]>([]);
  private nextId = 1;

  get activities(): Activity[] {
    return this._activities;
  }

  get hasActivity(): boolean {
    return this._activities.length > 0;
  }

  get currentActivity(): Activity | null {
    return this._activities[0] ?? null;
  }

  /**
   * Start tracking an activity. Returns a function to stop it.
   */
  start(label: string): () => void {
    const id = `activity-${this.nextId++}`;
    const activity: Activity = {
      id,
      label,
      startedAt: Date.now(),
    };
    this._activities = [...this._activities, activity];

    return () => this.stop(id);
  }

  /**
   * Stop a specific activity by ID
   */
  stop(id: string): void {
    this._activities = this._activities.filter((a) => a.id !== id);
  }

  /**
   * Clear all activities
   */
  clear(): void {
    this._activities = [];
  }
}

export const activityStore = new ActivityStore();

/**
 * Helper to wrap an async operation with activity tracking
 */
export async function withActivity<T>(label: string, fn: () => Promise<T>): Promise<T> {
  const stop = activityStore.start(label);
  try {
    return await fn();
  } finally {
    stop();
  }
}
