import { resolve } from 'path';
import chalk from 'chalk';
import { generateMustardJson } from './init.js';

export interface ConfigOptions {
  yes?: boolean;
}

export async function configCommand(options: ConfigOptions): Promise<void> {
  const projectPath = resolve(process.cwd());
  console.log(chalk.bold('\n🌿 Mustard — Git Flow Configuration\n'));
  await generateMustardJson(projectPath, options);
}
