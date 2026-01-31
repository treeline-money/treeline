/**
 * Auto-Tag Rules Service
 *
 * Handles CRUD operations for tag rules and rule matching logic.
 * Rules are stored in DuckDB (sys_transactions_rules table).
 */

import { executeQuery, executeQueryWithParams, type QueryParam } from "../../../sdk/api";
import type {
  TagRule,
  RuleCondition,
  RuleTestResult,
  DisplayConditions,
} from "./types";

/**
 * Generate a unique ID for a rule
 */
export function generateRuleId(): string {
  return `rule_${Date.now()}_${Math.random().toString(36).substr(2, 9)}`;
}

/**
 * Load all rules from database
 */
export async function loadRules(): Promise<TagRule[]> {
  const result = await executeQuery(`
    SELECT
      rule_id,
      name,
      sql_condition,
      display_conditions,
      tags,
      enabled,
      sort_order,
      created_at,
      updated_at
    FROM sys_transactions_rules
    ORDER BY sort_order ASC, created_at ASC
  `);

  return result.rows.map((row) => ({
    id: row[0] as string,
    name: row[1] as string,
    sqlCondition: row[2] as string,
    displayConditions: row[3] ? JSON.parse(row[3] as string) : undefined,
    tags: (row[4] as string[]) || [],
    enabled: row[5] as boolean,
    createdAt: row[7] ? new Date(row[7] as string).toISOString() : new Date().toISOString(),
    updatedAt: row[8] ? new Date(row[8] as string).toISOString() : new Date().toISOString(),
  }));
}

/**
 * Save a new rule
 *
 * sqlCondition is always required. displayConditions is optional for UI rendering.
 */
export async function saveRule(rule: TagRule): Promise<void> {
  // displayConditions is optional JSON for UI
  const displayConditionsJson = rule.displayConditions
    ? JSON.stringify(rule.displayConditions)
    : null;

  // Get max sort_order
  const maxResult = await executeQuery(`SELECT COALESCE(MAX(sort_order), -1) FROM sys_transactions_rules`);
  const sortOrder = ((maxResult.rows[0]?.[0] as number) || 0) + 1;

  // Build tags array for DuckDB
  const tagListSql = rule.tags.length > 0
    ? `list_value(${rule.tags.map(() => '?').join(', ')})`
    : `[]::VARCHAR[]`;

  // Use JS-computed timestamp to avoid ICU extension dependency
  const now = new Date().toISOString();

  // Params must match SQL placeholder order: id, name, sql_condition, display_conditions, [tags...], enabled, sort_order, created_at, updated_at
  const params: QueryParam[] = [
    rule.id,
    rule.name,
    rule.sqlCondition,
    displayConditionsJson,
    ...rule.tags,  // tags come before enabled/sort_order in SQL
    rule.enabled,
    sortOrder,
    now,
    now,
  ];

  await executeQueryWithParams(
    `
    INSERT INTO sys_transactions_rules
      (rule_id, name, sql_condition, display_conditions, tags, enabled, sort_order, created_at, updated_at)
    VALUES
      (?, ?, ?, ?, ${tagListSql}, ?, ?, ?::TIMESTAMP, ?::TIMESTAMP)
    `,
    params,
    { readonly: false }
  );
}

/**
 * Update an existing rule
 */
export async function updateRule(rule: TagRule): Promise<void> {
  // displayConditions is optional JSON for UI
  const displayConditionsJson = rule.displayConditions
    ? JSON.stringify(rule.displayConditions)
    : null;

  // Build tags array for DuckDB
  const tagListSql = rule.tags.length > 0
    ? `list_value(${rule.tags.map(() => '?').join(', ')})`
    : `[]::VARCHAR[]`;

  // Use JS-computed timestamp to avoid ICU extension dependency
  const now = new Date().toISOString();

  // Params must match SQL placeholder order: name, sql_condition, display_conditions, [tags...], enabled, updated_at, rule_id
  const params: QueryParam[] = [
    rule.name,
    rule.sqlCondition,
    displayConditionsJson,
    ...rule.tags,  // tags come before enabled in SQL
    rule.enabled,
    now,
    rule.id,
  ];

  await executeQueryWithParams(
    `
    UPDATE sys_transactions_rules SET
      name = ?,
      sql_condition = ?,
      display_conditions = ?,
      tags = ${tagListSql},
      enabled = ?,
      updated_at = ?::TIMESTAMP
    WHERE rule_id = ?
    `,
    params,
    { readonly: false }
  );
}

/**
 * Delete a rule by ID
 */
export async function deleteRule(ruleId: string): Promise<void> {
  await executeQueryWithParams(
    `DELETE FROM sys_transactions_rules WHERE rule_id = ?`,
    [ruleId],
    { readonly: false }
  );
}

/**
 * Toggle rule enabled state
 */
export async function toggleRuleEnabled(ruleId: string): Promise<void> {
  // Use JS-computed timestamp to avoid ICU extension dependency
  const now = new Date().toISOString();

  await executeQueryWithParams(
    `
    UPDATE sys_transactions_rules SET
      enabled = NOT enabled,
      updated_at = ?::TIMESTAMP
    WHERE rule_id = ?
    `,
    [now, ruleId],
    { readonly: false }
  );
}

/**
 * Check if a single condition matches a transaction
 */
function matchCondition(
  condition: RuleCondition,
  description: string,
  amount: number,
  accountName: string
): boolean {
  const { field, operator, value, value2 } = condition;

  // Get the field value to test
  let fieldValue: string | number;
  switch (field) {
    case "description":
      fieldValue = description.toLowerCase();
      break;
    case "amount":
      fieldValue = amount;
      break;
    case "account":
      fieldValue = accountName.toLowerCase();
      break;
    default:
      return false;
  }

  // Apply operator
  switch (operator) {
    case "contains":
      return typeof fieldValue === "string" && fieldValue.includes(String(value).toLowerCase());
    case "starts_with":
      return typeof fieldValue === "string" && fieldValue.startsWith(String(value).toLowerCase());
    case "ends_with":
      return typeof fieldValue === "string" && fieldValue.endsWith(String(value).toLowerCase());
    case "equals":
      if (typeof fieldValue === "string") {
        return fieldValue === String(value).toLowerCase();
      }
      return fieldValue === Number(value);
    case "regex":
      try {
        const regex = new RegExp(String(value), "i");
        return typeof fieldValue === "string" && regex.test(fieldValue);
      } catch {
        return false;
      }
    case "greater_than":
      return typeof fieldValue === "number" && fieldValue > Number(value);
    case "less_than":
      return typeof fieldValue === "number" && fieldValue < Number(value);
    case "between":
      return (
        typeof fieldValue === "number" &&
        fieldValue >= Number(value) &&
        fieldValue <= Number(value2 ?? value)
      );
    default:
      return false;
  }
}

/**
 * Check if a rule matches a transaction (using displayConditions if available)
 *
 * Note: This is used for in-memory matching. For database matching, use sqlCondition directly.
 */
export function matchesRule(
  rule: TagRule,
  description: string,
  amount: number,
  accountName: string
): boolean {
  if (!rule.enabled) {
    return false;
  }

  // If no displayConditions, we can't do in-memory matching
  if (!rule.displayConditions || rule.displayConditions.conditions.length === 0) {
    return false;
  }

  const results = rule.displayConditions.conditions.map((cond) =>
    matchCondition(cond, description, amount, accountName)
  );

  if (rule.displayConditions.logic === "all") {
    return results.every((r) => r);
  } else {
    return results.some((r) => r);
  }
}

/**
 * Get the WHERE clause for a rule
 * sqlCondition is always the canonical source
 */
export function getRuleWhereClause(rule: TagRule): string | null {
  if (rule.sqlCondition && rule.sqlCondition.trim()) {
    return rule.sqlCondition.trim();
  }
  return null;
}

/**
 * Build SQL WHERE clause from displayConditions
 * Used when creating/updating a rule via the UI builder
 */
export function buildSqlFromDisplayConditions(displayConditions: DisplayConditions): string | null {
  if (!displayConditions.conditions || displayConditions.conditions.length === 0) {
    return null;
  }

  const sqlParts: string[] = [];
  for (const cond of displayConditions.conditions) {
    const sql = conditionToSql(cond);
    if (sql) {
      sqlParts.push(sql);
    }
  }

  if (sqlParts.length === 0) {
    return null;
  }

  const logic = displayConditions.logic === "all" ? " AND " : " OR ";
  return sqlParts.join(logic);
}

/**
 * Test a rule against existing transactions in the database
 */
export async function testRule(rule: TagRule, limit: number = 10): Promise<RuleTestResult> {
  const whereClause = getRuleWhereClause(rule);

  if (!whereClause) {
    return { matchingCount: 0, sampleMatches: [] };
  }

  try {
    // Count total matches
    const countResult = await executeQuery(`
      SELECT COUNT(*) as cnt
      FROM transactions
      WHERE ${whereClause}
    `);
    const matchingCount = (countResult.rows[0]?.[0] as number) || 0;

    // Get sample matches
    const sampleResult = await executeQuery(`
      SELECT transaction_id, description, amount, account_name, transaction_date, tags
      FROM transactions
      WHERE ${whereClause}
      ORDER BY transaction_date DESC
      LIMIT ${limit}
    `);

    const sampleMatches = sampleResult.rows.map((row) => ({
      transaction_id: row[0] as string,
      description: row[1] as string,
      amount: row[2] as number,
      account_name: row[3] as string,
      transaction_date: row[4] as string,
      tags: (row[5] as string[]) || [],
    }));

    return { matchingCount, sampleMatches };
  } catch (e) {
    console.error("Failed to test rule:", e);
    return { matchingCount: 0, sampleMatches: [] };
  }
}

/**
 * Test a raw SQL WHERE clause against existing transactions
 */
export async function testSqlCondition(sqlCondition: string, limit: number = 10): Promise<RuleTestResult> {
  if (!sqlCondition.trim()) {
    return { matchingCount: 0, sampleMatches: [] };
  }

  try {
    // Count total matches
    const countResult = await executeQuery(`
      SELECT COUNT(*) as cnt
      FROM transactions
      WHERE ${sqlCondition}
    `);
    const matchingCount = (countResult.rows[0]?.[0] as number) || 0;

    // Get sample matches
    const sampleResult = await executeQuery(`
      SELECT transaction_id, description, amount, account_name, transaction_date, tags
      FROM transactions
      WHERE ${sqlCondition}
      ORDER BY transaction_date DESC
      LIMIT ${limit}
    `);

    const sampleMatches = sampleResult.rows.map((row) => ({
      transaction_id: row[0] as string,
      description: row[1] as string,
      amount: row[2] as number,
      account_name: row[3] as string,
      transaction_date: row[4] as string,
      tags: (row[5] as string[]) || [],
    }));

    return { matchingCount, sampleMatches };
  } catch (e) {
    console.error("Failed to test SQL condition:", e);
    throw e; // Re-throw so UI can show the error
  }
}


/**
 * Convert a single condition to SQL
 */
function conditionToSql(condition: RuleCondition): string | null {
  const { field, operator, value, value2 } = condition;

  // Map field to column name
  let column: string;
  switch (field) {
    case "description":
      column = "description";
      break;
    case "amount":
      column = "amount";
      break;
    case "account":
      column = "account_name";
      break;
    default:
      return null;
  }

  // Escape single quotes in string values
  const escapedValue = String(value).replace(/'/g, "''");

  switch (operator) {
    case "contains":
      return `LOWER(${column}) LIKE '%${escapedValue.toLowerCase()}%'`;
    case "starts_with":
      return `LOWER(${column}) LIKE '${escapedValue.toLowerCase()}%'`;
    case "ends_with":
      return `LOWER(${column}) LIKE '%${escapedValue.toLowerCase()}'`;
    case "equals":
      if (field === "amount") {
        return `${column} = ${Number(value)}`;
      }
      return `LOWER(${column}) = '${escapedValue.toLowerCase()}'`;
    case "regex":
      // DuckDB uses regexp_matches
      return `regexp_matches(LOWER(${column}), '${escapedValue.toLowerCase()}')`;
    case "greater_than":
      return `${column} > ${Number(value)}`;
    case "less_than":
      return `${column} < ${Number(value)}`;
    case "between":
      return `${column} BETWEEN ${Number(value)} AND ${Number(value2 ?? value)}`;
    default:
      return null;
  }
}

/**
 * Create a rule from a transaction description (extract pattern)
 */
export function createRuleFromTransaction(
  description: string,
  tags: string[],
  accountName?: string
): Partial<TagRule> {
  // Extract merchant pattern (similar to frequency suggester)
  const pattern = extractMerchantPattern(description);

  const conditions: RuleCondition[] = [];

  if (pattern) {
    conditions.push({
      field: "description",
      operator: "contains",
      value: pattern,
    });
  }

  // Build displayConditions for UI
  const displayConditions: DisplayConditions = {
    logic: "all",
    conditions,
  };

  // Generate SQL from display conditions
  const sqlCondition = buildSqlFromDisplayConditions(displayConditions) || "";

  return {
    id: generateRuleId(),
    name: pattern ? `Tag "${pattern}" as ${tags.join(", ")}` : `Tag as ${tags.join(", ")}`,
    sqlCondition,
    displayConditions,
    tags,
    enabled: true,
    createdAt: new Date().toISOString(),
    updatedAt: new Date().toISOString(),
  };
}

/**
 * Extract a merchant pattern from a transaction description.
 * Similar logic to FrequencyBasedSuggester.
 */
function extractMerchantPattern(description: string): string | null {
  if (!description) return null;

  // Clean up the description
  const cleaned = description
    .toUpperCase()
    .replace(/[*#]/g, " ")
    .replace(/\s+/g, " ")
    .trim();

  // Take first word as merchant pattern
  const words = cleaned.split(" ").filter((w) => w.length > 2);
  if (words.length === 0) return null;

  // Use first word, or first two if first is very short
  if (words[0].length <= 3 && words.length > 1) {
    return words.slice(0, 2).join(" ");
  }

  return words[0];
}

/**
 * Generate a SQL WHERE clause suggestion from transaction descriptions.
 * Analyzes the descriptions to find common patterns and suggests an appropriate clause.
 * For a single transaction, uses the full description for easier editing.
 */
export function generateSqlFromTransactions(descriptions: string[]): {
  sql: string;
  pattern: string | null;
  confidence: "high" | "medium" | "low";
} {
  if (descriptions.length === 0) {
    return { sql: "", pattern: null, confidence: "low" };
  }

  // For a single transaction, use the full description so user can edit it
  if (descriptions.length === 1) {
    const fullDesc = descriptions[0].trim();
    const escaped = fullDesc.replace(/'/g, "''");
    return {
      sql: `description ILIKE '%${escaped}%'`,
      pattern: fullDesc,
      confidence: "high",
    };
  }

  // For multiple transactions, extract patterns to find common merchant
  const patterns = descriptions.map(extractMerchantPattern).filter((p): p is string => p !== null);

  if (patterns.length === 0) {
    // No patterns found, use first description as-is
    const escaped = descriptions[0].replace(/'/g, "''");
    return {
      sql: `description ILIKE '%${escaped.toLowerCase()}%'`,
      pattern: descriptions[0],
      confidence: "low",
    };
  }

  // Find the most common pattern
  const patternCounts = new Map<string, number>();
  for (const p of patterns) {
    patternCounts.set(p, (patternCounts.get(p) || 0) + 1);
  }

  // Sort by count descending
  const sortedPatterns = [...patternCounts.entries()].sort((a, b) => b[1] - a[1]);
  const topPattern = sortedPatterns[0][0];
  const topCount = sortedPatterns[0][1];

  // Determine confidence based on how many descriptions share the pattern
  const confidence: "high" | "medium" | "low" =
    topCount === descriptions.length ? "high" : topCount > descriptions.length / 2 ? "medium" : "low";

  // Escape single quotes for SQL
  const escaped = topPattern.replace(/'/g, "''");

  return {
    sql: `description ILIKE '%${escaped.toLowerCase()}%'`,
    pattern: topPattern,
    confidence,
  };
}

/**
 * Get human-readable description of a condition
 */
export function describeCondition(condition: RuleCondition): string {
  const { field, operator, value, value2 } = condition;

  const fieldLabel = field === "description" ? "Description" : field === "amount" ? "Amount" : "Account";

  switch (operator) {
    case "contains":
      return `${fieldLabel} contains "${value}"`;
    case "starts_with":
      return `${fieldLabel} starts with "${value}"`;
    case "ends_with":
      return `${fieldLabel} ends with "${value}"`;
    case "equals":
      return `${fieldLabel} equals "${value}"`;
    case "regex":
      return `${fieldLabel} matches /${value}/`;
    case "greater_than":
      return `${fieldLabel} > ${value}`;
    case "less_than":
      return `${fieldLabel} < ${value}`;
    case "between":
      return `${fieldLabel} between ${value} and ${value2}`;
    default:
      return `${fieldLabel} ${operator} ${value}`;
  }
}

/**
 * Get human-readable description of a rule
 */
export function describeRule(rule: TagRule): string {
  // If we have displayConditions, use them for human-readable description
  if (rule.displayConditions && rule.displayConditions.conditions.length > 0) {
    const conditionDescs = rule.displayConditions.conditions.map(describeCondition);
    const logic = rule.displayConditions.logic === "all" ? " AND " : " OR ";
    return conditionDescs.join(logic);
  }

  // Otherwise, show the SQL condition
  if (rule.sqlCondition) {
    return rule.sqlCondition;
  }

  return "No conditions";
}

/**
 * Apply a rule's tags to all matching transactions in the database
 * Returns the number of transactions updated
 */
export async function applyRuleToExisting(rule: TagRule): Promise<number> {
  const whereClause = getRuleWhereClause(rule);

  if (!whereClause || rule.tags.length === 0) {
    return 0;
  }

  try {
    // Get all matching transactions (use view for querying)
    // Note: whereClause is user-controlled SQL (power-user feature), executed read-only
    const result = await executeQuery(`
      SELECT transaction_id, tags
      FROM transactions
      WHERE ${whereClause}
    `);

    if (result.rows.length === 0) {
      return 0;
    }

    // Update each transaction to add the rule's tags
    // Note: Must update sys_transactions (base table), not the transactions view
    let updatedCount = 0;
    for (const row of result.rows) {
      const transactionId = row[0] as string;
      const existingTags = (row[1] as string[]) || [];

      // Merge tags (avoid duplicates)
      const newTags = [...new Set([...existingTags, ...rule.tags])];

      // Only update if tags actually changed
      if (newTags.length !== existingTags.length || !newTags.every((t) => existingTags.includes(t))) {
        // Build parameterized tag list
        const tagListSql = newTags.length > 0
          ? `list_value(${newTags.map(() => '?').join(', ')})`
          : `[]::VARCHAR[]`;

        await executeQueryWithParams(
          `UPDATE sys_transactions
           SET tags = ${tagListSql}, tags_auto_applied = TRUE
           WHERE transaction_id = ?`,
          [...newTags, transactionId],
          { readonly: false }
        );
        updatedCount++;
      }
    }

    return updatedCount;
  } catch (e) {
    console.error("Failed to apply rule to existing transactions:", e);
    throw e;
  }
}

/**
 * Apply tags to matching transactions using a SQL condition
 * Returns the number of transactions updated
 */
export async function applyTagsToMatching(sqlCondition: string, tags: string[]): Promise<number> {
  if (!sqlCondition.trim() || tags.length === 0) {
    return 0;
  }

  try {
    // Get all matching transactions (use view for querying)
    // Note: sqlCondition is user-controlled SQL (power-user feature), executed read-only
    const result = await executeQuery(`
      SELECT transaction_id, tags
      FROM transactions
      WHERE ${sqlCondition}
    `);

    if (result.rows.length === 0) {
      return 0;
    }

    // Collect all transactions that need updating with their merged tags
    // Note: Must update sys_transactions (base table), not the transactions view
    const updates: { transactionId: string; newTags: string[] }[] = [];

    for (const row of result.rows) {
      const transactionId = row[0] as string;
      const existingTags = (row[1] as string[]) || [];

      // Merge tags (avoid duplicates)
      const newTags = [...new Set([...existingTags, ...tags])];

      // Only update if tags actually changed
      if (newTags.length !== existingTags.length || !newTags.every((t) => existingTags.includes(t))) {
        updates.push({ transactionId, newTags });
      }
    }

    if (updates.length === 0) {
      return 0;
    }

    // Escape a string for SQL (double up single quotes)
    const escapeSql = (s: string) => s.replace(/'/g, "''");

    // Build a single bulk UPDATE using UPDATE...FROM with VALUES
    // This is much faster than one query per transaction
    const valueRows = updates.map(({ transactionId, newTags }) => {
      const tagList = newTags.length > 0
        ? `list_value(${newTags.map(t => `'${escapeSql(t)}'`).join(', ')})`
        : `[]::VARCHAR[]`;
      return `('${escapeSql(transactionId)}', ${tagList})`;
    }).join(',\n        ');

    const ids = updates.map(u => `'${escapeSql(u.transactionId)}'`).join(', ');

    await executeQuery(
      `UPDATE sys_transactions AS t
       SET tags = v.new_tags, tags_auto_applied = TRUE
       FROM (VALUES
         ${valueRows}
       ) AS v(transaction_id, new_tags)
       WHERE t.transaction_id = v.transaction_id
         AND t.transaction_id IN (${ids})`,
      { readonly: false }
    );

    return updates.length;
  } catch (e) {
    console.error("Failed to apply tags to matching transactions:", e);
    throw e;
  }
}

/**
 * Apply all enabled auto-tag rules to a batch of imported transactions
 * Returns the total number of transactions that were tagged
 */
export async function applyRulesToBatch(batchId: string): Promise<number> {
  // Load all enabled rules
  const rules = await loadRules();
  const enabledRules = rules.filter(r => r.enabled && r.sqlCondition);

  if (enabledRules.length === 0) {
    return 0;
  }

  let totalTagged = 0;

  for (const rule of enabledRules) {
    const whereClause = getRuleWhereClause(rule);
    if (!whereClause || rule.tags.length === 0) {
      continue;
    }

    try {
      // Get matching transactions from this batch
      const result = await executeQueryWithParams(`
        SELECT transaction_id, tags
        FROM sys_transactions
        WHERE csv_batch_id = ?
          AND deleted_at IS NULL
          AND (${whereClause})
      `, [batchId]);

      if (result.rows.length === 0) {
        continue;
      }

      // Update each matching transaction to add the rule's tags
      for (const row of result.rows) {
        const transactionId = row[0] as string;
        const existingTags = (row[1] as string[]) || [];

        // Merge tags (avoid duplicates)
        const newTags = [...new Set([...existingTags, ...rule.tags])];

        // Only update if tags actually changed
        if (newTags.length !== existingTags.length || !newTags.every((t) => existingTags.includes(t))) {
          const tagListSql = newTags.length > 0
            ? `list_value(${newTags.map(() => '?').join(', ')})`
            : `[]::VARCHAR[]`;

          await executeQueryWithParams(
            `UPDATE sys_transactions
             SET tags = ${tagListSql}, tags_auto_applied = TRUE
             WHERE transaction_id = ?`,
            [...newTags, transactionId],
            { readonly: false }
          );
          totalTagged++;
        }
      }
    } catch (e) {
      // Log but don't fail the whole batch for one rule error
      console.error(`Failed to apply rule "${rule.name}" to batch:`, e);
    }
  }

  return totalTagged;
}
