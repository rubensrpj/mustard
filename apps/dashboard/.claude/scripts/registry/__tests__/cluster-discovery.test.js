'use strict';
// Tests for cluster-discovery.js — focuses on filename-cluster detection,
// the missing piece that left Next.js feature-folder patterns invisible.

const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');

const { discoverClusters } = require('../cluster-discovery');

function mkSubproject(layout) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'mustard-cluster-'));
  for (const [relPath, content] of Object.entries(layout)) {
    const full = path.join(dir, relPath);
    fs.mkdirSync(path.dirname(full), { recursive: true });
    fs.writeFileSync(full, content || '// stub\n', 'utf-8');
  }
  return dir;
}

test('filename-cluster: detail.tsx in 5 feature folders is detected', () => {
  const root = mkSubproject({
    'app/companies/_components/detail.tsx': 'export function CompaniesDetailView() {}\n',
    'app/users/_components/detail.tsx': 'export function UsersDetailView() {}\n',
    'app/tenants/_components/detail.tsx': 'export function TenantsDetailView() {}\n',
    'app/banks/_components/detail.tsx': 'export function BanksDetailView() {}\n',
    'app/parameters/_components/detail.tsx': 'export function ParametersDetailView() {}\n',
  });

  // Disable cache to get a clean read
  process.env.MUSTARD_CLUSTER_CACHE = 'off';
  const clusters = discoverClusters(root, 'typescript');

  const filenameCluster = clusters.find(c => c.kind === 'filename-cluster' && c.suffix === 'detail');
  assert.ok(filenameCluster, `expected filename-cluster for "detail" — got: ${clusters.map(c => `${c.kind}:${c.suffix}`).join(', ')}`);
  assert.equal(filenameCluster.fileCount, 5);
  assert.equal(filenameCluster.folders.length, 5);
  assert.match(filenameCluster.folderPattern, /detail\.tsx$/);
});

test('filename-cluster: structural basenames (page, layout, index) are skipped', () => {
  const root = mkSubproject({
    'app/users/page.tsx': '',
    'app/companies/page.tsx': '',
    'app/tenants/page.tsx': '',
    'app/banks/page.tsx': '',
    'app/users/layout.tsx': '',
    'app/companies/layout.tsx': '',
    'app/tenants/layout.tsx': '',
    'lib/utils/index.ts': '',
    'lib/forms/index.ts': '',
    'lib/auth/index.ts': '',
  });

  process.env.MUSTARD_CLUSTER_CACHE = 'off';
  const clusters = discoverClusters(root, 'typescript');

  assert.ok(!clusters.some(c => c.kind === 'filename-cluster' && c.suffix === 'page'),
    'page.tsx must be skipped (Next.js convention)');
  assert.ok(!clusters.some(c => c.kind === 'filename-cluster' && c.suffix === 'layout'),
    'layout.tsx must be skipped');
  assert.ok(!clusters.some(c => c.kind === 'filename-cluster' && c.suffix === 'index'),
    'index.ts must be skipped');
});

test('filename-cluster: basename below MIN_FILENAME_FOLDERS threshold is skipped', () => {
  const root = mkSubproject({
    // Only 2 folders — below default threshold of 3
    'app/users/_components/detail.tsx': '',
    'app/companies/_components/detail.tsx': '',
  });

  process.env.MUSTARD_CLUSTER_CACHE = 'off';
  const clusters = discoverClusters(root, 'typescript');
  assert.ok(!clusters.some(c => c.kind === 'filename-cluster'),
    'expected no filename-cluster — only 2 folders, below threshold');
});

test('filename-cluster: env override MUSTARD_FILENAME_MIN_FOLDERS works (subprocess for clean env)', () => {
  const root = mkSubproject({
    'app/a/detail.tsx': '',
    'app/b/detail.tsx': '',
  });

  const { spawnSync } = require('node:child_process');
  const r = spawnSync(process.execPath, [
    '-e',
    `
    process.env.MUSTARD_CLUSTER_CACHE = 'off';
    const { discoverClusters } = require(${JSON.stringify(require.resolve('../cluster-discovery'))});
    const clusters = discoverClusters(${JSON.stringify(root)}, 'typescript');
    process.stdout.write(JSON.stringify(clusters.filter(c => c.kind === 'filename-cluster')));
    `,
  ], {
    encoding: 'utf-8',
    env: { ...process.env, MUSTARD_FILENAME_MIN_FOLDERS: '2', MUSTARD_CLUSTER_CACHE: 'off' },
  });

  assert.equal(r.status, 0, `subprocess failed: ${r.stderr}`);
  const clusters = JSON.parse(r.stdout);
  assert.ok(clusters.some(c => c.suffix === 'detail'),
    `with threshold=2, 2 folders should suffice — got: ${JSON.stringify(clusters)}`);
});

test('PascalCase suffix-clusters still work (regression: backend patterns unchanged)', () => {
  const root = mkSubproject({
    'src/Services/UserService.cs': 'public class UserService {}\n',
    'src/Services/TenantService.cs': 'public class TenantService {}\n',
    'src/Services/BankService.cs': 'public class BankService {}\n',
    'src/Services/CompanyService.cs': 'public class CompanyService {}\n',
    'src/Services/AuthService.cs': 'public class AuthService {}\n',
  });

  process.env.MUSTARD_CLUSTER_CACHE = 'off';
  const clusters = discoverClusters(root, 'dotnet');

  const serviceCluster = clusters.find(c => c.suffix === 'Service');
  assert.ok(serviceCluster, 'Service cluster still detected');
  assert.ok(serviceCluster.fileCount >= 5);
});
