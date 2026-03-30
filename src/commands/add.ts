import { existsSync, mkdirSync, writeFileSync, readFileSync, readdirSync, statSync } from 'node:fs';
import { join, basename, relative, dirname } from 'node:path';
import { execSync } from 'node:child_process';
import { tmpdir } from 'node:os';

interface TemplateManifest {
  name: string;
  version: string;
  description?: string;
  files: string[];
  hooks_additions?: Array<{
    event: string;
    matcher: string;
    command: string;
    timeout: number;
  }>;
}

export interface AddOptions {
  force?: boolean;
}

/**
 * Add command - installs a community template into the current project's .claude/ directory.
 * Sources: GitHub (mustard-templates/{name}) or npm (mustard-template-{name}).
 */
export async function addCommand(templateSpec: string, options: AddOptions): Promise<void> {
  // Parse template spec: "template:name" or just "name"
  const name = templateSpec.replace(/^template:/, '');

  if (!name || name.includes('..') || /[^a-zA-Z0-9_-]/.test(name)) {
    console.error(`❌ Invalid template name: "${name}". Use alphanumeric, hyphens, underscores only.`);
    process.exit(1);
  }

  const cwd = process.cwd();
  const claudeDir = join(cwd, '.claude');

  if (!existsSync(claudeDir)) {
    console.error('❌ No .claude/ directory found. Run `mustard init` first.');
    process.exit(1);
  }

  console.log(`📦 Installing template: ${name}`);

  const tmpDir = join(tmpdir(), `mustard-template-${name}-${Date.now()}`);

  try {
    // Attempt 1: clone from GitHub
    const repoUrl = `https://github.com/mustard-templates/${name}.git`;
    console.log(`📥 Fetching from ${repoUrl}...`);

    let fetchedDir = tmpDir;

    try {
      execSync(`git clone --depth 1 "${repoUrl}" "${tmpDir}"`, {
        stdio: ['pipe', 'pipe', 'pipe'],
        timeout: 30000,
      });
    } catch {
      // Attempt 2: npm package
      console.log(`  GitHub repo not found. Trying npm: mustard-template-${name}...`);
      try {
        mkdirSync(tmpDir, { recursive: true });
        execSync(`npm pack mustard-template-${name} --pack-destination "${tmpDir}"`, {
          stdio: ['pipe', 'pipe', 'pipe'],
          timeout: 30000,
        });
        const tgz = readdirSync(tmpDir).find(f => f.endsWith('.tgz'));
        if (tgz) {
          execSync(`tar -xzf "${join(tmpDir, tgz)}" -C "${tmpDir}"`, {
            stdio: ['pipe', 'pipe', 'pipe'],
          });
          // npm pack extracts to a "package" subdirectory
          const packageDir = join(tmpDir, 'package');
          if (existsSync(packageDir)) {
            fetchedDir = packageDir;
          }
        }
      } catch {
        console.error(`❌ Template "${name}" not found on GitHub or npm.`);
        console.log('\nAvailable sources:');
        console.log(`  GitHub: github.com/mustard-templates/${name}`);
        console.log(`  npm:    mustard-template-${name}`);
        process.exit(1);
      }
    }

    // Read manifest
    const manifestPath = join(fetchedDir, 'mustard-template.json');
    let manifest: TemplateManifest;

    if (existsSync(manifestPath)) {
      manifest = JSON.parse(readFileSync(manifestPath, 'utf8')) as TemplateManifest;
    } else {
      // Auto-detect known .claude/ subdirectories
      manifest = {
        name,
        version: '0.0.0',
        files: detectFiles(fetchedDir),
      };
    }

    console.log(`📋 Template: ${manifest.name} v${manifest.version}`);
    if (manifest.description) console.log(`   ${manifest.description}`);

    // Copy files
    let copied = 0;
    let skipped = 0;

    for (const filePattern of manifest.files) {
      const src = join(fetchedDir, filePattern);
      if (!existsSync(src)) continue;

      const destBase = join(claudeDir, filePattern);

      if (statSync(src).isDirectory()) {
        const files = walkDir(src);
        for (const file of files) {
          const rel = relative(src, file);
          const dest = join(destBase, rel);

          if (existsSync(dest) && !options.force) {
            console.log(`  Skipping existing: ${join(filePattern, rel)}`);
            skipped++;
            continue;
          }

          mkdirSync(dirname(dest), { recursive: true });
          writeFileSync(dest, readFileSync(file));
          console.log(`  Copied: ${join(filePattern, rel)}`);
          copied++;
        }
      } else {
        if (existsSync(destBase) && !options.force) {
          console.log(`  Skipping existing: ${filePattern}`);
          skipped++;
          continue;
        }
        mkdirSync(dirname(destBase), { recursive: true });
        writeFileSync(destBase, readFileSync(src));
        console.log(`  Copied: ${filePattern}`);
        copied++;
      }
    }

    // Merge hook additions into settings.json
    if (manifest.hooks_additions && manifest.hooks_additions.length > 0) {
      const settingsPath = join(claudeDir, 'settings.json');
      if (existsSync(settingsPath)) {
        try {
          const settings = JSON.parse(readFileSync(settingsPath, 'utf8')) as Record<string, unknown>;
          const hooks = (settings.hooks ?? {}) as Record<string, unknown[]>;
          settings.hooks = hooks;

          for (const hook of manifest.hooks_additions) {
            const event = hook.event;
            if (!hooks[event]) hooks[event] = [];

            // Skip if hook command already registered
            const alreadyExists = (hooks[event] as Array<{ hooks?: Array<{ command?: string }> }>).some(h =>
              h.hooks?.some(hh => hh.command === hook.command)
            );

            if (!alreadyExists) {
              hooks[event].push({
                matcher: hook.matcher,
                hooks: [
                  {
                    type: 'command',
                    command: hook.command,
                    timeout: hook.timeout ?? 5,
                  },
                ],
              });
              console.log(`  Registered hook: ${event} -> ${basename(hook.command)}`);
            }
          }

          writeFileSync(settingsPath, JSON.stringify(settings, null, 2), 'utf8');
        } catch (err) {
          process.stderr.write(`  Could not merge hooks into settings.json: ${(err as Error).message}\n`);
        }
      }
    }

    console.log(`\nTemplate installed: ${copied} file(s) copied, ${skipped} skipped.`);
    if (skipped > 0) console.log('Use --force to overwrite existing files.');
  } finally {
    // Cleanup tmp directory
    try {
      execSync(`rm -rf "${tmpDir}"`, { stdio: ['pipe', 'pipe', 'pipe'] });
    } catch {
      try {
        execSync(`rmdir /s /q "${tmpDir}"`, { stdio: ['pipe', 'pipe', 'pipe'] });
      } catch {
        // Best-effort cleanup — ignore failure
      }
    }
  }
}

function detectFiles(dir: string): string[] {
  const knownSubdirs = ['commands', 'skills', 'hooks', 'context', 'scripts'];
  return knownSubdirs.filter(p => existsSync(join(dir, p)));
}

function walkDir(dir: string): string[] {
  const results: string[] = [];
  const entries = readdirSync(dir, { withFileTypes: true });
  for (const entry of entries) {
    const full = join(dir, entry.name);
    if (entry.isDirectory()) {
      if (entry.name === '.git' || entry.name === 'node_modules') continue;
      results.push(...walkDir(full));
    } else {
      results.push(full);
    }
  }
  return results;
}
