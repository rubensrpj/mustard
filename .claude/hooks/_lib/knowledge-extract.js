'use strict';
/**
 * KNOWLEDGE-EXTRACT: Shared pattern-detection logic for session-knowledge hooks.
 * Accepts pre-parsed state objects; caller is responsible for reading files.
 * @version 1.0.0
 */

/**
 * Extract candidate knowledge patterns from an array of pipeline state objects.
 *
 * @param {object[]} stateObjects  Parsed .pipeline-states/*.json objects.
 *                                 Each may have: specName, metrics.retries,
 *                                 metrics.apiCalls, metrics.toolBreakdown
 * @returns {{ type: string, name: string, description: string, source: string, tags: string[] }[]}
 */
function extractPatternsFromStates(stateObjects) {
  var patterns = [];

  for (var i = 0; i < stateObjects.length; i++) {
    var state = stateObjects[i];
    if (!state || typeof state !== 'object') continue;

    var metrics = state.metrics || {};
    var label = state.specName || state._file || 'unknown';

    // High retry count → lesson
    if (metrics.retries && metrics.retries > 2) {
      patterns.push({
        type: 'convention',
        name: 'high-retry-' + label,
        description: 'Pipeline required ' + metrics.retries + ' retries. Tool breakdown: ' +
          JSON.stringify(metrics.toolBreakdown || {}),
        source: 'session-knowledge',
        tags: ['retry', 'pipeline', 'lesson'],
      });
    }

    // Heavy tool usage → optimization pattern
    var totalCalls = metrics.apiCalls || 0;
    if (totalCalls > 50) {
      patterns.push({
        type: 'pattern',
        name: 'heavy-pipeline-' + label,
        description: 'Pipeline used ' + totalCalls + ' API calls. Consider splitting into smaller scope.',
        source: 'session-knowledge',
        tags: ['optimization', 'pipeline'],
      });
    }
  }

  return patterns;
}

module.exports = { extractPatternsFromStates };
