import { call } from '@/lib/api-client';

export function readEnv(repoPath: string): Promise<Record<string, string>> {
  return call<Record<string, string>>('dashboard_read_env', { repoPath });
}

export function writeEnv(repoPath: string, env: Record<string, string>): Promise<void> {
  return call<void>('dashboard_write_env', { repoPath, env });
}
