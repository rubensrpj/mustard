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
          if (m.gate_saves !== undefined) parts.push(`- Gate saves: ${m.gate_saves}`);
          if (m.wave_reentry !== undefined) parts.push(`- Wave reentries: ${m.wave_reentry}`);
          if (m.skillHits && Object.keys(m.skillHits).length > 0) {
            parts.push('- Skill hits:');
            for (const [agent, hits] of Object.entries(m.skillHits).sort()) {
              const pct = hits.loaded > 0 ? Math.round((hits.read / hits.loaded) * 100) + '%' : '\u2014';
              parts.push(`  - ${agent}: ${hits.read}/${hits.loaded} (${pct})`);
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

      // Gate & Quality metrics — aggregated across all completed pipelines
      {
        let totalGateSaves = 0;
        let totalWaveReentry = 0;
        const skillHitAgg = {}; // { agentType: { loaded: N, read: M } }
        let hasGateData = false;

        for (const f of files) {
          try {
            const m = JSON.parse(fs.readFileSync(path.join(metricsDir, f), 'utf8'));
            if (m.gate_saves !== undefined || m.wave_reentry !== undefined || m.skillHits) {
              hasGateData = true;
            }
            totalGateSaves += m.gate_saves || 0;
            totalWaveReentry += m.wave_reentry || 0;
            if (m.skillHits && typeof m.skillHits === 'object') {
              for (const [agent, hits] of Object.entries(m.skillHits)) {
                if (!skillHitAgg[agent]) skillHitAgg[agent] = { loaded: 0, read: 0 };
                skillHitAgg[agent].loaded += hits.loaded || 0;
                skillHitAgg[agent].read += hits.read || 0;
              }
            }
          } catch {}
        }

        parts.push('## Gate & Quality Metrics');
        parts.push('- Gate saves: ' + (hasGateData ? totalGateSaves : '\u2014') + (hasGateData ? ' (spec revisions after /approve)' : ''));
        parts.push('- Wave reentries: ' + (hasGateData ? totalWaveReentry : '\u2014') + (hasGateData ? ' (EXECUTE \u2192 PLAN)' : ''));
        parts.push('- Skill hit rate:');
        const agentKeys = Object.keys(skillHitAgg);
        if (agentKeys.length > 0) {
          parts.push('');
          parts.push('| Agent | Loaded | Read | Hit rate |');
          parts.push('|-------|--------|------|----------|');
          for (const agent of agentKeys.sort()) {
            const { loaded, read } = skillHitAgg[agent];
            const hitPct = loaded > 0 ? Math.round((read / loaded) * 100) + '%' : '\u2014';
            parts.push(`| ${agent} | ${loaded} | ${read} | ${hitPct} |`);
          }
        } else {
          parts.push('  (no skill tracking data yet)');
        }
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
