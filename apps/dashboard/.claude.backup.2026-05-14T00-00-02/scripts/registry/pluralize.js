'use strict';

/**
 * pluralize.js
 *
 * Single Responsibility: English pluralization helpers for converting
 * snake_case plural database table names to PascalCase singular entity names.
 *
 * Previously inlined in sync-registry.js. Extracted here so scanners can
 * reuse without depending on the top-level CLI script.
 *
 * Usage:
 *   const { snakeToPascalSingular, singularize, snakeToPascal } = require('./registry/pluralize');
 */

// ---------------------------------------------------------------------------
// IRREGULAR_PLURALS
// ---------------------------------------------------------------------------

/**
 * Lookup for common irregular plurals.
 * Key: lowercase plural form (snake_case table name or single word)
 * Value: PascalCase singular entity name
 */
const IRREGULAR_PLURALS = {
  people: 'Person',
  children: 'Child',
  men: 'Man',
  women: 'Woman',
  mice: 'Mouse',
  geese: 'Goose',
  teeth: 'Tooth',
  feet: 'Foot',
  data: 'Datum',
  indices: 'Index',
  matrices: 'Matrix',
  vertices: 'Vertex',
  analyses: 'Analysis',
  bases: 'Base',
  crises: 'Crisis',
  diagnoses: 'Diagnosis',
  hypotheses: 'Hypothesis',
  parentheses: 'Parenthesis',
  theses: 'Thesis',
  criteria: 'Criterion',
  phenomena: 'Phenomenon',
  media: 'Medium',
  statuses: 'Status',
  addresses: 'Address',
};

// ---------------------------------------------------------------------------
// singularize
// ---------------------------------------------------------------------------

/**
 * Singularize a single lowercase English word using simple heuristics.
 *
 * Examples:
 *   companies -> company
 *   addresses -> address
 *   boxes     -> box
 *   contracts -> contract
 *   queue     -> queue  (already singular)
 *
 * @param {string} word - lowercase word
 * @returns {string} - singular form (lowercase)
 */
function singularize(word) {
  // Check irregular
  if (IRREGULAR_PLURALS[word]) {
    return IRREGULAR_PLURALS[word].toLowerCase();
  }

  // Already-singular indicators
  if (
    word.endsWith('ss') ||
    word.endsWith('us') ||
    word.endsWith('is') ||
    word === 'queue'
  ) {
    return word;
  }

  // -ies -> -y (companies -> company, categories -> category)
  if (word.endsWith('ies')) {
    return word.slice(0, -3) + 'y';
  }

  // -sses -> -ss (addresses -> address)
  if (word.endsWith('sses')) {
    return word.slice(0, -2);
  }

  // -es after sh, ch, x, z -> remove -es (boxes -> box, churches -> church)
  if (
    word.endsWith('shes') ||
    word.endsWith('ches') ||
    word.endsWith('xes') ||
    word.endsWith('zes')
  ) {
    return word.slice(0, -2);
  }

  // Generic -s removal (contracts -> contract)
  if (word.endsWith('s') && !word.endsWith('ss')) {
    return word.slice(0, -1);
  }

  return word;
}

// ---------------------------------------------------------------------------
// snakeToPascalSingular
// ---------------------------------------------------------------------------

/**
 * Convert a snake_case plural table name to PascalCase singular entity name.
 *
 * Examples:
 *   contracts          -> Contract
 *   partner_types      -> PartnerType
 *   people             -> Person
 *   companies          -> Company
 *   product_categories -> ProductCategory
 *   email_queue        -> EmailQueue  (already singular)
 *
 * @param {string} snakePlural - snake_case plural name (e.g., 'partner_types')
 * @returns {string} - PascalCase singular entity name
 */
function snakeToPascalSingular(snakePlural) {
  // Check irregular lookup for the full compound name
  if (IRREGULAR_PLURALS[snakePlural]) {
    return IRREGULAR_PLURALS[snakePlural];
  }

  // Split by underscore; singularize only the LAST part (the noun)
  const parts = snakePlural.split('_');
  const result = parts.map((part, idx) => {
    const word = idx === parts.length - 1 ? singularize(part) : part;
    return word.charAt(0).toUpperCase() + word.slice(1);
  });

  return result.join('');
}

// ---------------------------------------------------------------------------
// snakeToPascal
// ---------------------------------------------------------------------------

/**
 * Convert a snake_case name to PascalCase (no singularization).
 *
 * Example:
 *   contract_status -> ContractStatus
 *
 * @param {string} snakeName
 * @returns {string}
 */
function snakeToPascal(snakeName) {
  return snakeName
    .split('_')
    .map(part => part.charAt(0).toUpperCase() + part.slice(1))
    .join('');
}

module.exports = { snakeToPascalSingular, singularize, snakeToPascal, IRREGULAR_PLURALS };
