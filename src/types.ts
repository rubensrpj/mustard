/**
 * Shared TypeScript types for Mustard CLI
 */

// ============== Stack & Project Types ==============

export interface Stack {
  name: string;
  version: string;
  path: string;
  files?: string[];
}

export interface NamingConventions {
  classes: string;
  files: string | Record<string, string>;
  variables?: string;
}

export interface StackScanResult {
  stacks: Stack[];
  packageManager: string;
  naming: NamingConventions;
}

export interface ArchitectureInfo {
  type: string;
  confidence: string;
  description?: string;
}

export interface Subproject {
  name: string;
  path: string;
}

export interface StructureInfo {
  name: string;
  type: 'monorepo' | 'single';
  architecture: ArchitectureInfo;
  directories: string[];
  folderStyle: string;
  subprojects: Subproject[];
}

export interface ProjectPatterns {
  classes: string;
  files: string | Record<string, string>;
  folders: string;
}

export interface DependencyInfo {
  /** subproject path → categorized libraries */
  [subprojectPath: string]: {
    frontend?: string[];
    backend?: string[];
    database?: string[];
    testing?: string[];
    tooling?: string[];
  };
}

export interface ProjectInfo {
  name: string;
  path: string;
  type: 'monorepo' | 'single';
  stacks: Stack[];
  patterns: ProjectPatterns;
  structure: StructureInfo;
  packageManager: string;
  entities: Entity[];
  dependencies: DependencyInfo;
  raw: {
    stack: StackScanResult;
    structure: StructureInfo;
  };
}

// ============== Analysis Types ==============

export interface Entity {
  name: string;
  file: string;
  type: string;
}

export interface Analysis {
  architecture: ArchitectureInfo;
  patterns: string[];
  naming?: NamingConventions;
  rules: string[];
  frameworks?: string[];
  entities: Entity[];
}

export interface CodeSample {
  file: string;
  content: string;
  type: string;
}

export interface CodeSamples {
  service?: CodeSample;
  endpoint?: CodeSample;
  hook?: CodeSample;
  component?: CodeSample;
  schema?: CodeSample;
}

// ============== Search Result Types ==============

export interface SearchResult {
  file_path: string;
  content: string;
  score?: number;
}

export interface SearchResponse {
  results: SearchResult[];
}

export interface TraceResult {
  callers?: SearchResult[];
  callees?: SearchResult[];
  graph?: Record<string, unknown>;
}

export interface DiscoveredPatterns {
  services: SearchResult[];
  repositories: SearchResult[];
  endpoints: SearchResult[];
  components: SearchResult[];
  hooks: SearchResult[];
  entities: Entity[];
  callGraph: TraceResult | null;
}

// ============== Generator Types ==============

export interface RegistryPatterns {
  db?: string;
  be?: string;
  fe?: string;
}

export interface RegistryMeta {
  version: string;
  generated: string;
  tool: string;
}

export interface EntityRegistry {
  _meta: RegistryMeta;
  _p: RegistryPatterns;
  e: Record<string, number>;
}

export interface GeneratedPrompts {
  orchestrator: string;
  bugfix: string;
  review: string;
  naming: string;
  backend?: string;
  frontend?: string;
  database?: string;
}

export interface GeneratedCommands {
  // Pipeline
  'feature': string;
  'bugfix': string;
  'approve': string;
  'complete': string;
  'resume': string;
  // Git
  'commit': string;
  'commit-push': string;
  'merge-main': string;
  // Validate
  'validate': string;
  'status': string;
  // Sync
  'sync-registry': string;
  'install-deps': string;
  'sync-context': string;
  // Report
  'report-daily': string;
  'report-weekly': string;
  // Scan
  'scan': string;
  // Task (L0 Universal Delegation)
  'task-analyze': string;
  'task-review': string;
  'task-refactor': string;
  'task-docs': string;
}

export interface GeneratedHooks {
  'enforce-pipeline.js': string;
  'enforce-grepai.js'?: string;
}

// ============== Options Types ==============

export interface InitOptions {
  force?: boolean;
  yes?: boolean;
  ollama?: boolean;
  grepai?: boolean;
  verbose?: boolean;
}

export interface SyncOptions {
  prompts?: boolean;
  context?: boolean;
  registry?: boolean;
  ollama?: boolean;
  grepai?: boolean;
  verbose?: boolean;
  force?: boolean;
}

export interface ScanOptions {
  verbose?: boolean;
}

export interface SearchOptions {
  limit?: number;
  format?: string;
  compact?: boolean;
}

export interface TraceOptions {
  format?: string;
  compact?: boolean;
  depth?: number;
}

export interface OllamaOptions {
  model?: string;
  format?: string;
  timeout?: number;
}

export interface GeneratorOptions {
  useOllama?: boolean;
  model?: string;
  hasGrepai?: boolean;
  verbose?: boolean;
  overwriteClaudeMd?: boolean;
  codeSamples?: CodeSamples;
}

export interface PromptGeneratorOptions {
  useOllama?: boolean;
  model?: string;
}

export interface DiscoverOptions {
  verbose?: boolean;
  stacks?: Stack[];
}

export interface CodeSampleOptions {
  maxLines?: number;
}

// ============== Dependencies Check ==============

export interface DependenciesCheck {
  ollama: boolean;
  ollamaModel: string | null;
  grepai: boolean;
}

// ============== Stack Pattern Types ==============

export interface StackPatternConfig {
  indicators: string[];
  configFiles?: string[];
  customScanner?: (projectPath: string) => Promise<Stack[]>;
  detector?: (path: string) => Promise<boolean>;
  versionExtractor?: (path: string) => Promise<string>;
}

export interface ArchitecturePatternConfig {
  folders?: string[];
  patterns?: RegExp[];
  confidence: 'high' | 'medium' | 'low';
}
