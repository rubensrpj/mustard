import { execSync } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';

interface ReviewOptions {
  ci?: boolean;
  pr?: string;
}

export async function reviewCommand(options: ReviewOptions): Promise<void> {
  const prNumber = options.pr;

  if (!prNumber) {
    console.error('Error: PR number required. Usage: mustard review --pr <number>');
    process.exit(1);
  }

  // Verify gh CLI is available
  try {
    execSync('gh --version', { stdio: 'ignore' });
  } catch {
    console.error('Error: GitHub CLI (gh) is required. Install from https://cli.github.com/');
    process.exit(1);
  }

  // Verify claude CLI is available
  try {
    execSync('claude --version', { stdio: 'ignore' });
  } catch {
    console.error('Error: Claude CLI is required. Install from https://docs.anthropic.com/');
    process.exit(1);
  }

  console.log(`Reviewing PR #${prNumber}${options.ci ? ' (CI mode)' : ''}...`);

  try {
    // Fetch PR info
    const prInfo = execSync(
      `gh pr view ${prNumber} --json title,body,additions,deletions,changedFiles,baseRefName,headRefName`,
      {
        encoding: 'utf8',
        timeout: 15000,
        stdio: ['pipe', 'pipe', 'pipe'],
        windowsHide: true,
      },
    );
    const pr = JSON.parse(prInfo);

    // Fetch PR diff
    const diff = execSync(`gh pr diff ${prNumber}`, {
      encoding: 'utf8',
      timeout: 30000,
      stdio: ['pipe', 'pipe', 'pipe'],
      windowsHide: true,
    });

    // Truncate diff if too large (max ~50k chars for Claude context)
    const maxDiffChars = 50000;
    let truncatedDiff = diff;
    if (diff.length > maxDiffChars) {
      truncatedDiff = diff.slice(0, maxDiffChars) + '\n\n... (diff truncated, showing first 50k chars)';
    }

    // Load guards if available
    let guards = '';
    const cwd = process.cwd();
    const guardsPath = join(cwd, '.claude', 'commands', 'guards.md');
    if (existsSync(guardsPath)) {
      try {
        guards = readFileSync(guardsPath, 'utf8').slice(0, 3000);
      } catch { /* non-critical — proceed without guards */ }
    }

    // Load CLAUDE.md for project rules
    let projectRules = '';
    const claudeMdPath = join(cwd, 'CLAUDE.md');
    if (existsSync(claudeMdPath)) {
      try {
        projectRules = readFileSync(claudeMdPath, 'utf8').slice(0, 2000);
      } catch { /* non-critical — proceed without project rules */ }
    }

    // Build review prompt
    const prompt = buildReviewPrompt(pr, truncatedDiff, guards, projectRules);

    // Run Claude for review (both modes use --print for non-interactive output)
    const reviewResult = execSync(`claude --print "${escapeForShell(prompt)}"`, {
      encoding: 'utf8',
      timeout: 120000,
      maxBuffer: 10 * 1024 * 1024,
      stdio: ['pipe', 'pipe', 'pipe'],
      windowsHide: true,
    });

    console.log('\nReview Result:\n');
    console.log(reviewResult);

    // In CI mode, post review as PR comment
    if (options.ci && reviewResult.trim()) {
      try {
        const commentBody = `## Automated Review (Mustard)\n\n${reviewResult}`;
        execSync(`gh pr comment ${prNumber} --body "${escapeForShell(commentBody)}"`, {
          stdio: ['pipe', 'pipe', 'pipe'],
          timeout: 15000,
          windowsHide: true,
        });
        console.log(`\nReview posted as comment on PR #${prNumber}`);
      } catch (err) {
        process.stderr.write(`Warning: Could not post review comment: ${(err as Error).message}\n`);
      }

      // Exit with code based on review severity
      if (/\bCRITICAL\b/i.test(reviewResult)) {
        console.log('Critical issues found.');
        process.exit(1);
      }
    }

    console.log('Review complete.');

  } catch (err) {
    console.error(`Error: Review failed: ${(err as Error).message}`);
    process.exit(1);
  }
}

function buildReviewPrompt(pr: Record<string, unknown>, diff: string, guards: string, projectRules: string): string {
  const additions = pr.additions as number;
  const deletions = pr.deletions as number;
  const changedFiles = pr.changedFiles as number;
  const title = pr.title as string;
  const body = pr.body as string | undefined;
  const baseRefName = pr.baseRefName as string;
  const headRefName = pr.headRefName as string;

  const parts = [
    'Review this pull request for code quality, security, and correctness.',
    '',
    `## PR: ${title}`,
    `Base: ${baseRefName} <- Head: ${headRefName}`,
    `Changes: +${additions} -${deletions} (${changedFiles} files)`,
    '',
  ];

  if (body) {
    parts.push('## Description', body, '');
  }

  if (projectRules) {
    parts.push('## Project Rules', projectRules, '');
  }

  if (guards) {
    parts.push("## Guards (DO/DON'T rules)", guards, '');
  }

  parts.push(
    '## Review Checklist',
    '- [ ] No security vulnerabilities (injection, XSS, secrets)',
    '- [ ] Code follows project conventions',
    '- [ ] No unnecessary complexity',
    '- [ ] Error handling is appropriate',
    '- [ ] No breaking changes without migration',
    '',
    '## Diff',
    '```diff',
    diff,
    '```',
    '',
    'Provide a structured review with:',
    '1. **Summary**: What this PR does (1-2 sentences)',
    '2. **Issues**: List of issues found (CRITICAL / WARNING / INFO)',
    '3. **Suggestions**: Improvements (optional)',
    '4. **Verdict**: APPROVE / REQUEST_CHANGES / COMMENT',
  );

  return parts.join('\n');
}

function escapeForShell(str: string): string {
  // Escape for double-quoted shell string
  return str
    .replace(/\\/g, '\\\\')
    .replace(/"/g, '\\"')
    .replace(/\$/g, '\\$')
    .replace(/`/g, '\\`');
}
