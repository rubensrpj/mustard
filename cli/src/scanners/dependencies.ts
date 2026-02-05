import { readFile } from 'fs/promises';
import { join } from 'path';
import { glob } from 'glob';
import type { DependencyInfo } from '../types.js';

/** Notable npm packages grouped by category */
const NPM_CATEGORIES: Record<string, string[]> = {
  frontend: [
    'react', 'react-dom', 'next', 'vue', 'nuxt', 'svelte', '@sveltejs/kit',
    'angular', '@angular/core',
    // UI
    '@radix-ui', 'shadcn', '@headlessui', '@mui/material', 'antd',
    'tailwindcss', 'styled-components', '@emotion/react',
    'lucide-react', '@heroicons/react', 'react-icons',
    // State
    '@tanstack/react-query', 'swr', 'zustand', 'jotai', 'recoil',
    'redux', '@reduxjs/toolkit', 'mobx', 'pinia', 'vuex',
    // Forms
    'react-hook-form', '@hookform/resolvers', 'formik',
    // Tables
    '@tanstack/react-table', '@tanstack/react-virtual', 'ag-grid-react',
    // Validation (shared)
    'zod', 'yup', 'joi', 'valibot',
    // Charts
    'recharts', 'chart.js', 'd3', 'visx', 'nivo',
    // Date
    'date-fns', 'dayjs', 'moment', 'luxon',
    // Animation
    'framer-motion', 'lottie-react', 'gsap',
    // Notifications
    'sonner', 'react-toastify', 'react-hot-toast', 'notistack',
  ],
  backend: [
    'express', 'fastify', 'koa', 'hapi', 'nest', '@nestjs/core',
    'graphql', '@apollo/server', 'mercurius',
    'trpc', '@trpc/server',
    'passport', 'jsonwebtoken', 'bcrypt', 'argon2',
    'nodemailer', 'bullmq', 'agenda',
    'winston', 'pino',
    'swagger-jsdoc', 'swagger-ui-express',
  ],
  database: [
    'drizzle-orm', 'drizzle-kit',
    'prisma', '@prisma/client',
    'typeorm', 'sequelize', 'knex', 'kysely',
    'mongoose', 'mongodb',
    'pg', 'mysql2', 'better-sqlite3', 'redis', 'ioredis',
  ],
  testing: [
    'jest', 'vitest', '@testing-library/react', '@testing-library/jest-dom',
    'playwright', '@playwright/test', 'cypress',
    'supertest', 'msw', 'nock',
  ],
  tooling: [
    'typescript', 'eslint', 'prettier', 'biome',
    'vite', 'turbo', 'nx', 'webpack', 'esbuild', 'tsup', 'tsx',
    'husky', 'lint-staged', 'commitlint',
    '@kubb/cli', 'openapi-typescript',
  ],
};

/** Notable NuGet packages grouped by category */
const NUGET_CATEGORIES: Record<string, string[]> = {
  backend: [
    'Microsoft.AspNetCore', 'FastEndpoints',
    'MediatR', 'AutoMapper', 'Mapster',
    'FluentValidation', 'FluentResults',
    'Serilog', 'NLog',
    'Swashbuckle', 'NSwag',
    'MassTransit', 'Rebus',
    'Hangfire', 'Quartz',
    'HotChocolate', 'GraphQL',
    'IdentityServer', 'Duende',
    'Polly', 'Refit',
  ],
  database: [
    'Microsoft.EntityFrameworkCore', 'Npgsql',
    'Dapper', 'SqlKata',
    'MongoDB.Driver', 'StackExchange.Redis',
  ],
  testing: [
    'xunit', 'NUnit', 'MSTest',
    'Moq', 'NSubstitute', 'FakeItEasy',
    'FluentAssertions', 'Shouldly',
    'Bogus', 'AutoFixture',
    'WireMock', 'Testcontainers',
  ],
};

/**
 * Scan all package.json and .csproj files to extract real dependencies
 */
export async function scanDependencies(
  projectPath: string,
  options: { verbose?: boolean } = {}
): Promise<DependencyInfo> {
  const result: DependencyInfo = {};

  // Scan npm packages
  const pkgFiles = await glob('**/package.json', {
    cwd: projectPath,
    nodir: true,
    ignore: ['**/node_modules/**', '**/dist/**', '**/.next/**']
  });

  for (const pkgFile of pkgFiles) {
    try {
      const fullPath = join(projectPath, pkgFile);
      const pkg = JSON.parse(await readFile(fullPath, 'utf-8')) as {
        name?: string;
        dependencies?: Record<string, string>;
        devDependencies?: Record<string, string>;
      };

      const allDeps = { ...pkg.dependencies, ...pkg.devDependencies };
      const subPath = pkgFile.replace(/[/\\]package\.json$/, '') || '.';

      const categorized = categorizeNpmDeps(allDeps);
      if (hasAnyDeps(categorized)) {
        result[subPath] = categorized;
        if (options.verbose) {
          const total = Object.values(categorized).reduce((sum, arr) => sum + (arr?.length ?? 0), 0);
          console.log(`  Found ${total} notable dependencies in ${subPath}/package.json`);
        }
      }
    } catch { /* skip unreadable */ }
  }

  // Scan .csproj files for NuGet packages
  const csprojFiles = await glob('**/*.csproj', {
    cwd: projectPath,
    nodir: true,
    ignore: ['**/bin/**', '**/obj/**']
  });

  for (const csprojFile of csprojFiles) {
    try {
      const fullPath = join(projectPath, csprojFile);
      const content = await readFile(fullPath, 'utf-8');
      const subPath = csprojFile.replace(/[/\\][^/\\]+\.csproj$/, '') || '.';

      const categorized = categorizeNugetDeps(content);
      if (hasAnyDeps(categorized)) {
        // Merge with existing entry for this path
        const existing = result[subPath] ?? {};
        for (const [cat, libs] of Object.entries(categorized)) {
          const key = cat as keyof typeof categorized;
          if (libs && libs.length > 0) {
            existing[key] = [...(existing[key] ?? []), ...libs];
          }
        }
        result[subPath] = existing;
        if (options.verbose) {
          const total = Object.values(categorized).reduce((sum, arr) => sum + (arr?.length ?? 0), 0);
          console.log(`  Found ${total} notable packages in ${csprojFile}`);
        }
      }
    } catch { /* skip */ }
  }

  return result;
}

function categorizeNpmDeps(deps: Record<string, string>): DependencyInfo[string] {
  const result: DependencyInfo[string] = {};
  const depNames = Object.keys(deps);

  for (const [category, patterns] of Object.entries(NPM_CATEGORIES)) {
    const found: string[] = [];
    for (const pattern of patterns) {
      // Match exact name or prefix (e.g., '@radix-ui' matches '@radix-ui/react-dialog')
      const matches = depNames.filter(d =>
        d === pattern || d.startsWith(pattern + '/')
      );
      for (const match of matches) {
        const version = deps[match]?.replace(/[\^~>=<]*/g, '') ?? '';
        // For scoped packages that match a prefix, just note the prefix once
        if (pattern.startsWith('@') && match !== pattern && match.startsWith(pattern + '/')) {
          if (!found.some(f => f.startsWith(pattern))) {
            found.push(`${pattern}/* ${version}`);
          }
        } else {
          found.push(`${match} ${version}`);
        }
      }
    }
    if (found.length > 0) {
      result[category as keyof typeof result] = [...new Set(found)];
    }
  }

  return result;
}

function categorizeNugetDeps(csprojContent: string): DependencyInfo[string] {
  const result: DependencyInfo[string] = {};

  // Extract PackageReference elements
  const packageRefs = [...csprojContent.matchAll(/<PackageReference\s+Include="([^"]+)"(?:\s+Version="([^"]+)")?/g)];
  const packages = packageRefs.map(m => ({ name: m[1] ?? '', version: m[2] ?? '' }));

  for (const [category, patterns] of Object.entries(NUGET_CATEGORIES)) {
    const found: string[] = [];
    for (const pattern of patterns) {
      const matches = packages.filter(p =>
        p.name === pattern || p.name.startsWith(pattern + '.')
      );
      for (const match of matches) {
        found.push(`${match.name} ${match.version}`);
      }
    }
    if (found.length > 0) {
      result[category as keyof typeof result] = [...new Set(found)];
    }
  }

  return result;
}

function hasAnyDeps(categorized: DependencyInfo[string]): boolean {
  return Object.values(categorized).some(arr => arr && arr.length > 0);
}
