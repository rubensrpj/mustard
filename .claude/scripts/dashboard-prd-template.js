'use strict';

const TYPE_LABEL = { feature: 'Feature', bugfix: 'Bugfix' };

function generatePrdMarkdown(input) {
  const type = (input.type || '').toLowerCase();
  if (!TYPE_LABEL[type]) {
    throw new Error(`type must be 'feature' or 'bugfix' (got ${input.type})`);
  }
  const slug = String(input.slug || '').trim();
  if (!slug) throw new Error('slug is required');
  const title = String(input.title || '').trim();
  if (!title) throw new Error('title is required');
  const summary = String(input.summary || '').trim();
  if (!summary) throw new Error('summary is required');

  const scope = input.scope === 'light' ? 'light' : 'full';
  const checkpoint = new Date().toISOString();
  const boundaries = arr(input.boundaries);
  const checklist = arr(input.checklist);
  const ac = Array.isArray(input.acceptanceCriteria) ? input.acceptanceCriteria : [];
  const decisions = arr(input.decisionsNotObvious);
  const nonGoals = arr(input.nonGoals);
  const why = String(input.why || '').trim();

  if (boundaries.length === 0) throw new Error('at least one boundary is required');
  if (checklist.length === 0) throw new Error('at least one checklist item is required');
  if (ac.length === 0) {
    console.warn('[dashboard-prd] WARNING: spec generated without Acceptance Criteria');
  }

  const project = String(input.project || '').trim();

  const lines = [];
  lines.push(`# ${TYPE_LABEL[type]}: ${slug}`);
  lines.push(`### Status: draft | Phase: PLAN | Scope: ${scope}`);
  lines.push(`### Checkpoint: ${checkpoint}`);
  if (project && project !== '(root)') {
    lines.push(`### Project: ${project}`);
  }
  lines.push('');
  lines.push('## Summary');
  lines.push(summary);
  lines.push('');

  if (why) {
    lines.push('## Why');
    lines.push(why);
    lines.push('');
  }

  lines.push('## Boundaries');
  for (const b of boundaries) lines.push(`- ${b}`);
  lines.push('');

  lines.push('## Checklist');
  for (const c of checklist) lines.push(`- [ ] ${c}`);
  lines.push('');

  if (ac.length > 0) {
    lines.push('## Acceptance Criteria');
    ac.forEach((entry, i) => {
      const desc = String(entry && entry.description || '').trim() || `Criterion ${i + 1}`;
      const cmd = String(entry && entry.command || '').trim();
      lines.push(`${i + 1}. ${desc}`);
      if (cmd) {
        lines.push('   ```');
        for (const ln of cmd.split(/\r?\n/)) lines.push(`   ${ln}`);
        lines.push('   ```');
      }
    });
    lines.push('');
  }

  if (decisions.length > 0) {
    lines.push('## Decisões não-óbvias');
    for (const d of decisions) lines.push(`- ${d}`);
    lines.push('');
  }

  if (nonGoals.length > 0) {
    lines.push('## Non-Goals');
    for (const n of nonGoals) lines.push(`- ${n}`);
    lines.push('');
  }

  return lines.join('\n').replace(/\n{3,}/g, '\n\n');
}

function slugify(s) {
  return String(s || '')
    .normalize('NFKD')
    .replace(/[̀-ͯ]/g, '')
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '')
    .slice(0, 60);
}

function arr(v) {
  if (!Array.isArray(v)) return [];
  return v.map(x => String(x || '').trim()).filter(Boolean);
}

module.exports = { generatePrdMarkdown, slugify };
