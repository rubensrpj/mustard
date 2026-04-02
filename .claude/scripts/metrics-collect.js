#!/usr/bin/env node
/**
 * METRICS-COLLECT: Collect and display pipeline metrics
 *
 * Reads metrics from:
 * - Active pipeline states (.pipeline-states/*.json)
 * - Completed pipeline archives (.claude/metrics/*.json)
 * - RTK gain data (if available)
 *
 * Output: Formatted markdown to stdout
 *
 * @version 1.0.0
 */

const { execSync } = require('child_process');
const fs = require('fs');
const path = require('path');

function main() {
  const cwd = process.cwd();
  const claudeDir = path.join(cwd, '.claude');
  const parts = [];

  parts.push('# Pipeline Metrics');
  parts.push('');

  // Active pipelines
  const statesDir = path.join(claudeDir, '.pipeline-states');
  if (fs.existsSync(statesDir)) {
    const files = fs.readdirSync(statesDir).filter(f => f.endsWith('.json'));
    for (const f of files) {
      try {
        const state = JSON.parse(fs.readFileSync(path.join(statesDir, f), 'utf8'));
        if (state.metrics) {
          const name = f.replace('.json', '');
          const m = state.metrics;
          const duration = m.startedAt ? formatDuration(new Date(m.startedAt), new Date()) : 'unknown';
          parts.push(`## Active: ${name}`);
          parts.push(`- Duration: ${duration}`);
          parts.push(`- API calls: ${m.apiCalls || 0}`);
          parts.push(`- Retries: ${m.retries || 0}`);
          if (m.toolBreakdown && Object.keys(m.toolBreakdown).length > 0) {
            parts.push('- Tool breakdown:');
            for (const [tool, count] of Object.entries(m.toolBreakdown).sort((a, b) => b[1] - a[1])) {
              parts.push(`  - ${tool}: ${count}`);
            }
          }
          parts.push('');
        }
      } catch {}
    }
  }

  // Completed pipelines (archived metrics)
  const metricsDir = path.join(claudeDir, 'metrics');
  if (fs.existsSync(metricsDir)) {
    const files = fs.readdirSync(metricsDir).filter(f => f.endsWith('.json'));
    if (files.length > 0) {
      parts.push('## Completed Pipelines');
      parts.push('');

      let totalCalls = 0;
      let totalRetries = 0;
      let totalDurationMs = 0;
      let count = 0;

      // Show last 10
      const sorted = files.sort().reverse().slice(0, 10);
      for (const f of sorted) {
        try {
          const m = JSON.parse(fs.readFileSync(path.join(metricsDir, f), 'utf8'));
          const name = f.replace('.json', '');
          const duration = m.durationMs ? formatMs(m.durationMs) : 'unknown';
          parts.push(`### ${name}`);
          parts.push(`- Duration: ${duration}`);
          parts.push(`- API calls: ${m.apiCalls || 0}`);
          parts.push(`- Retries: ${m.retries || 0}`);
          if (m.rtkSavings) {
            parts.push(`- RTK savings: ${m.rtkSavings.pct}% (${Math.round((m.rtkSavings.saved || 0) / 1000)}k tokens)`);
          }
          parts.push('');

          totalCalls += m.apiCalls || 0;
          totalRetries += m.retries || 0;
          totalDurationMs += m.durationMs || 0;
          count++;
        } catch {}
      }

      if (count > 0) {
        parts.push('## Averages (last ' + count + ' pipelines)');
        parts.push(`- Avg duration: ${formatMs(Math.round(totalDurationMs / count))}`);
        parts.push(`- Avg API calls: ${Math.round(totalCalls / count)}`);
        parts.push(`- Avg retries: ${Math.round(totalRetries / count)}`);
        parts.push('');
      }

      // Pass@1 metrics — computed across ALL completed pipeline files (not just last 10)
      var pass1Count = 0;
      var totalPipelines = 0;
      var totalRetrySum = 0;
      for (var i = 0; i < files.length; i++) {
        try {
          var m = JSON.parse(fs.readFileSync(path.join(metricsDir, files[i]), 'utf8'));
          totalPipelines++;
          totalRetrySum += (m.retries || 0);
          if ((m.retries || 0) === 0) pass1Count++;
        } catch {}
      }
      if (totalPipelines > 0) {
        var pass1Pct = Math.round((pass1Count / totalPipelines) * 100);
        var avgRetries = (totalRetrySum / totalPipelines).toFixed(1);
        parts.push('## Pass@1 Metrics');
        parts.push('- Pass@1: ' + pass1Pct + '% (' + pass1Count + '/' + totalPipelines + ' completed without retries)');
        parts.push('- Avg retries per pipeline: ' + avgRetries);
        parts.push('');
      }
    }
  }

  // RTK total savings
  try {
    const raw = execSync('rtk gain --all --format json', {
      encoding: 'utf8',
      timeout: 3000,
      stdio: ['pipe', 'pipe', 'pipe'],
      windowsHide: true,
    });
    const rtk = JSON.parse(raw);
    const saved = rtk.saved_tokens ?? rtk.savedTokens ?? 0;
    const pct = rtk.savings_pct ?? rtk.savingsPct ?? 0;
    if (saved > 0) {
      parts.push('## RTK Token Economy');
      parts.push(`- Total saved: ${Math.round(saved / 1000)}k tokens`);
      parts.push(`- Savings rate: ${Math.round(pct)}%`);
      parts.push('');
    }
  } catch {} // RTK not available

  if (parts.length <= 2) {
    parts.push('No metrics data found. Run a pipeline first.');
  }

  console.log(parts.join('\n'));
  process.exit(0);
}

function formatDuration(start, end) {
  const ms = end.getTime() - start.getTime();
  return formatMs(ms);
}

function formatMs(ms) {
  if (ms < 60000) return `${Math.round(ms / 1000)}s`;
  const m = Math.floor(ms / 60000);
  const s = Math.round((ms % 60000) / 1000);
  if (m < 60) return `${m}m${s}s`;
  const h = Math.floor(m / 60);
  return `${h}h${m % 60}m`;
}

main();
