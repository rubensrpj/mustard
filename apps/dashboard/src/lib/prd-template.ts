export interface PrdInput {
  type: 'feature' | 'bugfix';
  slug: string;
  title: string;
  summary: string;
  why?: string;
  scope: 'light' | 'full';
  boundaries: string[];
  checklist: string[];
  acceptanceCriteria: { title: string; command: string }[];
  decisionsNotObvious?: string[];
  nonGoals?: string[];
  project?: string;
}

export function slugify(text: string): string {
  return text
    .toLowerCase()
    .trim()
    .replace(/[^a-z0-9-]/g, '-')
    .replace(/-+/g, '-')
    .replace(/^-|-$/g, '');
}

export function generatePrdMarkdown(input: PrdInput): string {
  const { type, slug, title, summary, why, scope, boundaries, checklist, acceptanceCriteria, decisionsNotObvious, nonGoals } = input;

  if (!slug) throw new Error('campo obrigatório: slug');
  if (!title) throw new Error('campo obrigatório: title');
  if (!summary) throw new Error('campo obrigatório: summary');
  if (boundaries.length === 0) throw new Error('campo obrigatório: boundaries');
  if (checklist.length === 0) throw new Error('campo obrigatório: checklist');

  const kind = type === 'feature' ? 'Feature' : 'Bugfix';
  const now = new Date().toISOString();

  const lines: string[] = [
    `# ${kind}: ${slug}`,
    '',
    `### Status: draft | Phase: PLAN | Scope: ${scope}`,
    `### Checkpoint: ${now}`,
    '',
    '## Summary',
    summary,
  ];

  if (why) {
    lines.push('', '## Por quê?', why);
  }

  lines.push('', '## Boundaries');
  for (const b of boundaries) {
    lines.push(`- ${b}`);
  }

  lines.push('', '## Checklist');
  for (const c of checklist) {
    lines.push(`- [ ] ${c}`);
  }

  lines.push('', '## Acceptance Criteria');
  if (acceptanceCriteria.length === 0) {
    lines.push('- [ ] AC-1: (adicione critérios de aceite)');
  } else {
    acceptanceCriteria.forEach((ac, i) => {
      lines.push(`- [ ] AC-${i + 1}: ${ac.title} — Command: \`${ac.command}\``);
    });
  }

  if (decisionsNotObvious && decisionsNotObvious.length > 0) {
    lines.push('', '## Decisões não-óbvias');
    for (const d of decisionsNotObvious) {
      lines.push(`- ${d}`);
    }
  }

  if (nonGoals && nonGoals.length > 0) {
    lines.push('', '## Non-Goals');
    for (const g of nonGoals) {
      lines.push(`- ${g}`);
    }
  }

  return lines.join('\n');
}
