// runtime-shim type declarations (CommonJS module). Bun-only since Mustard 2.0.

export interface RuntimeInfo {
  kind: 'bun';
  version: string;
  bunSqliteAvailable: true;
}

export declare function pickRuntime(): RuntimeInfo;
export declare function isBun(): true;
export declare function loadSqlite(): any | null;
