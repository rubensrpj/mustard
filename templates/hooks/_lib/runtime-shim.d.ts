// runtime-shim type declarations (CommonJS module).
// Kept TS3+-compatible: no template literal types, no satisfies, no const generics.

export interface RuntimeInfo {
  kind: 'bun' | 'node';
  version: string;
  bunSqliteAvailable: boolean;
}

export declare function pickRuntime(): RuntimeInfo;
export declare function isBun(): boolean;
export declare function isNode(): boolean;
export declare function loadSqlite(): any | null;
