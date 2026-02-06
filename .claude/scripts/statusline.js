#!/usr/bin/env node
const { execSync } = require('child_process');
const path = require('path');

let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', chunk => input += chunk);
process.stdin.on('end', () => {
  try {
    const data = JSON.parse(input);

    // Model name
    const model = data.model?.display_name || 'Claude';

    // Current directory
    const cwd = data.workspace?.current_dir || data.cwd || process.cwd();
    const projectDir = data.workspace?.project_dir || '';

    // Determine module name (generic approach)
    let module = path.basename(cwd);
    if (projectDir && cwd !== projectDir) {
      const relPath = cwd.replace(projectDir, '').replace(/^[/\\]/, '');
      // Use first two path segments for context
      const segments = relPath.split(/[/\\]/).filter(Boolean);
      if (segments.length >= 2) {
        module = `${segments[0]}/${segments[1]}`;
      } else if (segments.length === 1) {
        module = segments[0];
      }
    }

    // Git info
    let gitInfo = '';
    try {
      const branch = execSync('git rev-parse --abbrev-ref HEAD', { cwd, encoding: 'utf8', stdio: ['pipe', 'pipe', 'pipe'] }).trim();
      if (branch) {
        const status = execSync('git status --porcelain', { cwd, encoding: 'utf8', stdio: ['pipe', 'pipe', 'pipe'] });
        let statusStr = '';
        if (status) {
          const lines = status.split('\n').filter(l => l);
          const staged = lines.filter(l => /^[MADRC]/.test(l)).length;
          const modified = lines.filter(l => /^.[MD]/.test(l)).length;
          const untracked = lines.filter(l => l.startsWith('??')).length;
          const parts = [];
          if (staged > 0) parts.push(`+${staged}`);
          if (modified > 0) parts.push(`~${modified}`);
          if (untracked > 0) parts.push(`?${untracked}`);
          statusStr = parts.length > 0 ? `\x1b[31m${parts.join('')}\x1b[0m` : '\x1b[32m✓\x1b[0m';
        } else {
          statusStr = '\x1b[32m✓\x1b[0m';
        }
        gitInfo = ` \x1b[36m${branch}\x1b[0m ${statusStr}`;
      }
    } catch {}

    // Context info
    let ctxInfo = '';
    const ctxRem = data.context_window?.remaining_percentage;
    if (ctxRem != null) {
      const color = ctxRem < 20 ? '\x1b[31m' : ctxRem < 50 ? '\x1b[33m' : '\x1b[32m';
      const totalTokens = (data.context_window?.total_input_tokens || 0) + (data.context_window?.total_output_tokens || 0);
      const tokensK = Math.floor(totalTokens / 1000);
      ctxInfo = ` ${color}${Math.round(ctxRem)}%\x1b[0m \x1b[90m(${tokensK}k)\x1b[0m`;

      const cacheRead = data.context_window?.current_usage?.cache_read_input_tokens || 0;
      if (cacheRead > 0) {
        ctxInfo += ` \x1b[96m⚡${Math.floor(cacheRead / 1000)}k\x1b[0m`;
      }
    }

    // Agent info
    let agentInfo = '';
    const agent = data.agent?.name;
    if (agent) {
      agentInfo = ` \x1b[35m[${agent}]\x1b[0m`;
    }

    // Output
    console.log(`\x1b[1m${module}\x1b[0m${gitInfo} \x1b[90m|\x1b[0m \x1b[34m${model}\x1b[0m${ctxInfo}${agentInfo}`);
  } catch (e) {
    console.log('Claude');
  }
});
