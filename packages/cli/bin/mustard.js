#!/usr/bin/env bun

if (typeof Bun === 'undefined' && !process.versions.bun) {
  process.stderr.write(
    'mustard: Bun runtime required (>= 1.2.0).\n' +
    'Install: https://bun.sh  |  Windows: scoop install bun  |  Unix: curl -fsSL https://bun.sh/install | bash\n'
  );
  process.exit(1);
}

import { run } from '../dist/cli.js';

run();
