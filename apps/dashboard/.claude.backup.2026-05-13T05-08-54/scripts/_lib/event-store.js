#!/usr/bin/env node
'use strict';
/**
 * EVENT-STORE (scripts side): Thin re-export of templates/hooks/_lib/event-store.js.
 *
 * Scripts (dashboard, metrics-collect, complete-spec) consume the EventStore
 * via the same singleton resolver the hooks use. Keeping one resolver avoids
 * drift in findUp() logic between hook/script contexts.
 *
 * Fail-open: returns null on any error. Callers MUST fall back to legacy
 * filesystem reads (events.jsonl, .pipeline-states/*.metrics.json, metrics/*.json).
 *
 * @version 1.0.0
 */
module.exports = require('../../hooks/_lib/event-store.js');
