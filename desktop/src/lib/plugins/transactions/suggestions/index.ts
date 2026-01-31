/**
 * Tag Suggestions Module
 *
 * Export the suggester interface and implementations.
 */

export type { TagSuggester, TagSuggestion, Transaction } from "./types";
export { FrequencyBasedSuggester } from "./frequency";
