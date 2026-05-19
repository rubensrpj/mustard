import chalk from 'chalk';
import ora from 'ora';
import inquirer from 'inquirer';

import * as npmService from '../services/npm.js';

export interface AutoUpdateOptions {
  checkOnly?: boolean;
  yes?: boolean;
}

/**
 * Auto-update command - checks for updates and installs latest version
 */
export async function autoUpdateCommand(options: AutoUpdateOptions): Promise<void> {
  console.log(chalk.bold('\n🌿 Mustard CLI - Auto Update\n'));

  // Check for updates
  const spinner = ora('Checking for updates...').start();

  let updateInfo: { hasUpdate: boolean; current: string; latest: string };
  try {
    updateInfo = await npmService.checkForUpdate();
    spinner.stop();
  } catch (error) {
    spinner.fail('Failed to check for updates');
    const message = error instanceof Error ? error.message : 'Unknown error';
    console.error(chalk.red(`  ${message}`));
    return;
  }

  const { hasUpdate, current, latest } = updateInfo;

  console.log(chalk.white(`  Current version: ${chalk.cyan(current)}`));
  console.log(chalk.white(`  Latest version:  ${chalk.cyan(latest)}`));
  console.log();

  if (!hasUpdate) {
    console.log(chalk.green('✅ You are running the latest version!\n'));
    return;
  }

  console.log(chalk.yellow(`⬆️  Update available: ${current} → ${latest}\n`));

  if (options.checkOnly) {
    console.log(chalk.gray('  Run "mustard auto-update" to install the update.\n'));
    return;
  }

  // Confirm update
  if (!options.yes) {
    const { proceed } = await inquirer.prompt<{ proceed: boolean }>([
      {
        type: 'confirm',
        name: 'proceed',
        message: 'Install update now?',
        default: true
      }
    ]);

    if (!proceed) {
      console.log(chalk.yellow('\n⚠️  Cancelled.\n'));
      return;
    }
  }

  // Install update
  const updateSpinner = ora('Installing update...').start();

  try {
    await npmService.updateGlobal();
    updateSpinner.succeed('Update installed');
    console.log(chalk.green.bold(`\n✅ Updated to v${latest}!\n`));
    console.log(chalk.gray('  Run "mustard update" in your projects to update .claude/ files.\n'));
  } catch (error) {
    updateSpinner.fail('Update failed');
    const message = error instanceof Error ? error.message : 'Unknown error';
    console.error(chalk.red(`  ${message}`));
  }
}
