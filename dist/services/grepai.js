import { execSync } from 'child_process';
/**
 * Check if grepai CLI is available
 */
export async function checkGrepaiAvailable() {
    try {
        execSync('grepai --version', { stdio: 'pipe' });
        return true;
    }
    catch {
        return false;
    }
}
/**
 * Get grepai index status
 */
export async function indexStatus() {
    try {
        const result = execSync('grepai index-status --format json', {
            encoding: 'utf-8',
            maxBuffer: 10 * 1024 * 1024
        });
        return JSON.parse(result);
    }
    catch {
        return null;
    }
}
/**
 * Search using grepai semantic search
 */
export async function search(query, options = {}) {
    const { limit = 10, format = 'json', compact = false } = options;
    try {
        const compactFlag = compact ? '--compact' : '';
        const result = execSync(`grepai search "${query}" --limit ${limit} --format ${format} ${compactFlag}`, {
            encoding: 'utf-8',
            maxBuffer: 10 * 1024 * 1024
        });
        return JSON.parse(result);
    }
    catch (error) {
        const message = error instanceof Error ? error.message : 'Unknown error';
        console.error('grepai search failed:', message);
        return { results: [] };
    }
}
/**
 * Find all functions that call the specified symbol
 */
export async function traceCallers(symbol, options = {}) {
    const { format = 'json', compact = false } = options;
    try {
        const compactFlag = compact ? '--compact' : '';
        const result = execSync(`grepai trace-callers "${symbol}" --format ${format} ${compactFlag}`, { encoding: 'utf-8' });
        return JSON.parse(result);
    }
    catch {
        return { callers: [] };
    }
}
/**
 * Find all functions called by the specified symbol
 */
export async function traceCallees(symbol, options = {}) {
    const { format = 'json', compact = false } = options;
    try {
        const compactFlag = compact ? '--compact' : '';
        const result = execSync(`grepai trace-callees "${symbol}" --format ${format} ${compactFlag}`, { encoding: 'utf-8' });
        return JSON.parse(result);
    }
    catch {
        return { callees: [] };
    }
}
/**
 * Build a complete call graph around a symbol
 */
export async function traceGraph(symbol, options = {}) {
    const { format = 'json', depth = 2 } = options;
    try {
        const result = execSync(`grepai trace-graph "${symbol}" --depth ${depth} --format ${format}`, { encoding: 'utf-8' });
        return JSON.parse(result);
    }
    catch {
        return { graph: {} };
    }
}
//# sourceMappingURL=grepai.js.map