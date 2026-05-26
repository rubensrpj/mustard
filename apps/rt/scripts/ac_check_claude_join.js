// AC-TF1: zero `.join(".claude")` em apps/rt/src/ fora de ClaudePaths/tests/claude_paths.rs
//
// Brace-tracker contract:
//   - `#[cfg(test)]` or `#[test]` arms test scope.
//   - The NEXT `{` after the attribute (which may be on a later line — eg
//     `#[cfg(test)]\nmod tests {`) opens the test region; we then track
//     `{`/`}` and leave the region when depth returns to zero.
//
// Exemption marker:
//   - A line containing `// ClaudePaths-exempt` is treated as documented
//     out-of-scope and not flagged. Use this for `~/.claude/` (user-global)
//     references that are intentionally outside the per-project `ClaudePaths`
//     contract.
const fs = require('fs');
const path = require('path');

function walk(dir, files = []) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) walk(full, files);
    else if (entry.name.endsWith('.rs')) files.push(full);
  }
  return files;
}

const root = 'apps/rt/src';
const violations = [];
const rxJoinClaude = /\.join\("\.claude"/;
const rxExempt = /\/\/\s*ClaudePaths-exempt/;

for (const file of walk(root)) {
  if (file.includes('claude_paths.rs')) continue;
  const lines = fs.readFileSync(file, 'utf8').split(/\r?\n/);
  let pendingTestAttr = false; // saw `#[cfg(test)]`/`#[test]`, waiting for first `{`
  let inTest = false;
  let braceDepth = 0;
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    if (/^\s*#\[cfg\(test\)\]/.test(line) || /^\s*#\[test\]/.test(line)) {
      pendingTestAttr = true;
    }
    const opens = (line.match(/\{/g) || []).length;
    const closes = (line.match(/\}/g) || []).length;
    if (pendingTestAttr && opens > 0) {
      // Enter test region on the first `{` after the attribute. The opening
      // brace counts toward depth.
      inTest = true;
      pendingTestAttr = false;
      braceDepth += opens - closes;
      if (braceDepth <= 0) {
        inTest = false; // one-line test body (rare but possible)
      }
      continue;
    }
    if (inTest) {
      braceDepth += opens - closes;
      if (braceDepth <= 0) {
        inTest = false;
        // The closing line is still inside the test region — skip violation
        // check for it.
        continue;
      }
    }
    if (rxExempt.test(line)) continue;
    if (/ClaudePaths/.test(line)) continue;
    if (inTest) continue;
    if (rxJoinClaude.test(line)) {
      violations.push(`${file}:${i + 1}: ${line.trim()}`);
    }
  }
}

if (violations.length > 0) {
  console.log(`FAIL: ${violations.length} violations of AC-TF1`);
  console.log(violations.slice(0, 50).join('\n'));
  if (violations.length > 50) console.log(`...and ${violations.length - 50} more`);
  process.exit(1);
} else {
  console.log('PASS: AC-TF1 — zero .join(".claude") in apps/rt/src non-test code');
}
