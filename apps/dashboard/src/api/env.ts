import { invoke } from '@tauri-apps/api/core';

export function readEnv(repoPath: string): Promise<Record<string, string>> {
  return invoke<Record<string, string>>('dashboard_read_env', { repoPath });
}

export function writeEnv(repoPath: string, env: Record<string, string>): Promise<void> {
  return invoke<void>('dashboard_write_env', { repoPath, env });
}
