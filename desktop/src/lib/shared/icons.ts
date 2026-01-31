/**
 * Shared icon utilities
 *
 * Maps emoji icons (used in plugin manifests) to Icon component names.
 * Also supports direct icon names for modern plugins.
 */

/**
 * Mapping from emoji to Icon component name.
 * Used by Sidebar and TabBar to display consistent icons.
 */
export const emojiToIconName: Record<string, string> = {
  "🏦": "bank",
  "💰": "wallet",
  "💳": "credit-card",
  "🏷": "tag",
  "⚡": "zap",
  "⚙": "settings",
  "📊": "chart",
  "🔍": "search",
  "🎯": "target",
  "🔄": "repeat",
  "🛡": "shield",
};

/**
 * Known icon names that can be used directly in plugin manifests.
 */
const validIconNames = new Set([
  "bank", "wallet", "credit-card", "chart", "tag", "tags",
  "piggy-bank", "target", "repeat", "shield",
  "database", "refresh", "link", "zap",
  "settings", "command", "search",
  "palette", "info", "play", "arrow-right", "log-out", "beaker",
  "check", "x", "chevron-down", "chevron-right",
  "plus", "minus", "edit", "trash", "eye", "external-link", "pin",
  "activity", "trending-up", "package",
]);

/**
 * Convert an emoji or icon name to an Icon component name.
 * - If input is a valid icon name, returns it directly
 * - If input is an emoji, maps it to icon name
 * - Returns "database" as fallback for unknown values
 */
export function getIconName(iconOrEmoji: string): string {
  // If it's already a valid icon name, use it directly
  if (validIconNames.has(iconOrEmoji)) {
    return iconOrEmoji;
  }
  // Otherwise try emoji mapping
  return emojiToIconName[iconOrEmoji] || "database";
}
