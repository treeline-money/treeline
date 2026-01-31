/**
 * Frequency-Based Tag Suggester
 *
 * Loads all tag data ONCE into memory, then computes suggestions purely in-memory.
 * No DB queries during navigation - only on initial load/refresh.
 */

import { executeQuery } from "../../../sdk";
import type { TagSuggester, TagSuggestion, Transaction } from "./types";

interface TagData {
  globalFrequencies: Map<string, number>;
  merchantTagFrequencies: Map<string, Map<string, number>>; // merchant pattern -> tag -> count
}

export class FrequencyBasedSuggester implements TagSuggester {
  private tagData: TagData | null = null;
  private loadingPromise: Promise<TagData> | null = null;

  /**
   * Load all tag data into memory. Call this once at startup.
   * Returns cached data if already loaded.
   */
  async loadTagData(): Promise<TagData> {
    if (this.tagData) {
      return this.tagData;
    }

    // Avoid duplicate loads
    if (this.loadingPromise) {
      return this.loadingPromise;
    }

    this.loadingPromise = this.fetchTagData();
    this.tagData = await this.loadingPromise;
    this.loadingPromise = null;
    return this.tagData;
  }

  /**
   * Force refresh of tag data from DB.
   */
  async refresh(): Promise<void> {
    this.tagData = null;
    this.loadingPromise = null;
    await this.loadTagData();
  }

  /**
   * Get all known tags sorted by frequency (most frequent first).
   * Returns empty array if tag data hasn't been loaded yet.
   */
  getAllTags(): string[] {
    if (!this.tagData) return [];
    return Array.from(this.tagData.globalFrequencies.entries())
      .sort((a, b) => b[1] - a[1])
      .map(([tag]) => tag);
  }

  private async fetchTagData(): Promise<TagData> {
    const globalFrequencies = new Map<string, number>();
    const merchantTagFrequencies = new Map<string, Map<string, number>>();

    try {
      // Optimized: Use aggregation queries instead of fetching all rows
      // This returns ~50-100 rows instead of potentially 20-30K

      // Query 1: Global tag frequencies
      const globalResult = await executeQuery(`
        WITH unnested AS (
          SELECT UNNEST(tags) as tag
          FROM transactions
          WHERE tags IS NOT NULL AND len(tags) > 0
        )
        SELECT tag, COUNT(*) as cnt
        FROM unnested
        GROUP BY tag
      `);

      for (const row of globalResult.rows) {
        const tag = row[0] as string;
        const count = row[1] as number;
        globalFrequencies.set(tag, count);
      }

      // Query 2: Merchant-specific tag frequencies
      // Extract first significant word as merchant pattern (uppercase, skip short words)
      const merchantResult = await executeQuery(`
        WITH base AS (
          SELECT
            CASE
              WHEN LENGTH(SPLIT_PART(UPPER(REGEXP_REPLACE(description, '[*#]+', ' ', 'g')), ' ', 1)) <= 3
                AND LENGTH(TRIM(SPLIT_PART(UPPER(REGEXP_REPLACE(description, '[*#]+', ' ', 'g')), ' ', 2))) > 0
              THEN TRIM(SPLIT_PART(UPPER(REGEXP_REPLACE(description, '[*#]+', ' ', 'g')), ' ', 1)) || ' ' ||
                   TRIM(SPLIT_PART(UPPER(REGEXP_REPLACE(description, '[*#]+', ' ', 'g')), ' ', 2))
              ELSE TRIM(SPLIT_PART(UPPER(REGEXP_REPLACE(description, '[*#]+', ' ', 'g')), ' ', 1))
            END as merchant,
            UNNEST(tags) as tag
          FROM transactions
          WHERE tags IS NOT NULL AND len(tags) > 0
            AND LENGTH(TRIM(description)) > 2
        )
        SELECT merchant, tag, COUNT(*) as cnt
        FROM base
        WHERE LENGTH(merchant) > 2
        GROUP BY merchant, tag
      `);

      for (const row of merchantResult.rows) {
        const merchant = row[0] as string;
        const tag = row[1] as string;
        const count = row[2] as number;

        let merchantTags = merchantTagFrequencies.get(merchant);
        if (!merchantTags) {
          merchantTags = new Map();
          merchantTagFrequencies.set(merchant, merchantTags);
        }
        merchantTags.set(tag, count);
      }
    } catch (e) {
      console.error("Failed to load tag data:", e);
    }

    return { globalFrequencies, merchantTagFrequencies };
  }

  async suggestBatch(
    transactions: Transaction[],
    limit: number
  ): Promise<Map<string, TagSuggestion[]>> {
    const result = new Map<string, TagSuggestion[]>();

    if (transactions.length === 0) {
      return result;
    }

    // Ensure tag data is loaded
    const tagData = await this.loadTagData();

    // Compute suggestions for each transaction (pure in-memory)
    for (const txn of transactions) {
      const suggestions = this.computeSuggestionsForTransaction(txn, tagData, limit);
      result.set(txn.transaction_id, suggestions);
    }

    return result;
  }

  /**
   * Compute suggestions for a single transaction. Pure in-memory, no DB access.
   */
  private computeSuggestionsForTransaction(
    txn: Transaction,
    tagData: TagData,
    limit: number
  ): TagSuggestion[] {
    const existingTags = new Set(txn.tags || []);
    const scores = new Map<string, { score: number; reason: string }>();

    // Merchant-based suggestions (highest weight)
    const pattern = this.extractMerchantPattern(txn.description);
    if (pattern) {
      const merchantTags = tagData.merchantTagFrequencies.get(pattern);
      if (merchantTags) {
        for (const [tag, count] of merchantTags) {
          if (!existingTags.has(tag)) {
            const merchantScore = count * 3; // 3x weight for merchant match
            scores.set(tag, { score: merchantScore, reason: "similar merchant" });
          }
        }
      }
    }

    // Global frequency (fallback, lower weight)
    for (const [tag, count] of tagData.globalFrequencies) {
      if (!existingTags.has(tag)) {
        const existing = scores.get(tag);
        if (existing) {
          existing.score += count;
        } else {
          scores.set(tag, { score: count, reason: "frequently used" });
        }
      }
    }

    // Sort by score and take top N
    return Array.from(scores.entries())
      .map(([tag, { score, reason }]) => ({ tag, score, reason }))
      .sort((a, b) => b.score - a.score)
      .slice(0, limit);
  }

  /**
   * Extract a merchant pattern from a transaction description.
   * Takes the first significant word(s) as the merchant identifier.
   */
  private extractMerchantPattern(description: string): string | null {
    if (!description) return null;

    // Clean up the description
    const cleaned = description
      .toUpperCase()
      .replace(/[*#]/g, " ")
      .replace(/\s+/g, " ")
      .trim();

    // Take first word as merchant pattern
    const words = cleaned.split(" ").filter(w => w.length > 2);
    if (words.length === 0) return null;

    // Use first word, or first two if first is very short
    if (words[0].length <= 3 && words.length > 1) {
      return words.slice(0, 2).join(" ");
    }

    return words[0];
  }
}
