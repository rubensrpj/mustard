import type { InitOptions } from '../types.js';
export interface UpdateOptions extends InitOptions {
    includeClaudeMd?: boolean;
}
/**
 * Update command - updates Mustard core files while preserving client customizations
 */
export declare function updateCommand(options: UpdateOptions): Promise<void>;
