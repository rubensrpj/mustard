import * as grepai from '../services/grepai.js';
/**
 * Semantic search patterns for different aspects of a codebase
 */
const SEARCH_QUERIES = {
    architecture: [
        'service layer business logic implementation',
        'repository pattern data access',
        'API endpoint route handler',
        'controller action method'
    ],
    patterns: [
        'dependency injection constructor',
        'DTO data transfer object mapping',
        'middleware request pipeline',
        'validation rules validator'
    ],
    entities: [
        'entity class definition database',
        'database table schema model',
        'domain model aggregate root'
    ],
    frontend: [
        'React component with hooks and state',
        'custom hook data fetching',
        'form validation submit handler',
        'context provider state management'
    ]
};
/**
 * Discover patterns using semantic search
 */
export async function discoverPatterns(options = {}) {
    const { verbose = false, stacks = [] } = options;
    const results = {
        services: [],
        repositories: [],
        endpoints: [],
        components: [],
        hooks: [],
        entities: [],
        callGraph: null
    };
    // Backend patterns
    const hasBackend = stacks.some(s => ['dotnet', 'node', 'python', 'java', 'go'].includes(s.name));
    if (hasBackend) {
        // Search for services
        const serviceResults = await grepai.search('service layer business logic implementation', { limit: 5 });
        results.services = serviceResults.results ?? [];
        // Search for repositories
        const repoResults = await grepai.search('repository pattern data access', { limit: 5 });
        results.repositories = repoResults.results ?? [];
        // Search for endpoints
        const endpointResults = await grepai.search('API endpoint route handler', { limit: 5 });
        results.endpoints = endpointResults.results ?? [];
        // Search for entities
        const entityResults = await grepai.search('entity class definition database model', { limit: 10 });
        results.entities = (entityResults.results ?? [])
            .map(r => {
            const name = extractEntityName(r);
            if (!name)
                return null;
            return {
                name,
                file: r.file_path,
                type: inferEntityType(r)
            };
        })
            .filter((e) => e !== null);
    }
    // Frontend patterns
    const hasFrontend = stacks.some(s => ['react', 'nextjs', 'vue', 'angular'].includes(s.name));
    if (hasFrontend) {
        // Search for components
        const componentResults = await grepai.search('React component with hooks and state', { limit: 5 });
        results.components = componentResults.results ?? [];
        // Search for hooks
        const hookResults = await grepai.search('custom hook data fetching useQuery', { limit: 5 });
        results.hooks = hookResults.results ?? [];
    }
    // Build call graph for main service if found
    if (results.services.length > 0 && results.services[0]) {
        const mainService = extractSymbolName(results.services[0]);
        if (mainService) {
            results.callGraph = await grepai.traceGraph(mainService, { depth: 2 });
        }
    }
    return results;
}
/**
 * Discover entities in the codebase
 */
export async function discoverEntities(stacks = []) {
    const entities = [];
    // Search for entity definitions
    const entityResults = await grepai.search('entity class definition table schema', { limit: 20 });
    if (entityResults.results) {
        for (const result of entityResults.results) {
            const name = extractEntityName(result);
            if (name && !entities.find(e => e.name === name)) {
                entities.push({
                    name,
                    file: result.file_path,
                    type: inferEntityType(result)
                });
            }
        }
    }
    return entities;
}
/**
 * Get code samples for each pattern type
 */
export async function getCodeSamples(patterns, options = {}) {
    const { maxLines = 50 } = options;
    const samples = {};
    // Get service sample
    if (patterns.services && patterns.services.length > 0 && patterns.services[0]) {
        samples.service = {
            file: patterns.services[0].file_path,
            content: truncateContent(patterns.services[0].content, maxLines),
            type: 'service'
        };
    }
    // Get endpoint sample
    if (patterns.endpoints && patterns.endpoints.length > 0 && patterns.endpoints[0]) {
        samples.endpoint = {
            file: patterns.endpoints[0].file_path,
            content: truncateContent(patterns.endpoints[0].content, maxLines),
            type: 'endpoint'
        };
    }
    // Get hook sample
    if (patterns.hooks && patterns.hooks.length > 0 && patterns.hooks[0]) {
        samples.hook = {
            file: patterns.hooks[0].file_path,
            content: truncateContent(patterns.hooks[0].content, maxLines),
            type: 'hook'
        };
    }
    // Get component sample
    if (patterns.components && patterns.components.length > 0 && patterns.components[0]) {
        samples.component = {
            file: patterns.components[0].file_path,
            content: truncateContent(patterns.components[0].content, maxLines),
            type: 'component'
        };
    }
    return samples;
}
/**
 * Extract symbol name from search result
 */
function extractSymbolName(result) {
    if (!result || !result.content)
        return null;
    // Try to extract class name
    const classMatch = result.content.match(/class\s+(\w+)/);
    if (classMatch?.[1])
        return classMatch[1];
    // Try to extract function name
    const funcMatch = result.content.match(/(?:function|const|let|var)\s+(\w+)/);
    if (funcMatch?.[1])
        return funcMatch[1];
    return null;
}
/**
 * Extract entity name from search result
 */
function extractEntityName(result) {
    if (!result || !result.content)
        return null;
    // .NET entity
    const classMatch = result.content.match(/class\s+(\w+)(?:\s*:\s*\w+)?/);
    if (classMatch?.[1])
        return classMatch[1];
    // Drizzle schema
    const drizzleMatch = result.content.match(/export\s+const\s+(\w+)\s*=/);
    if (drizzleMatch?.[1])
        return drizzleMatch[1];
    // Python model
    const pythonMatch = result.content.match(/class\s+(\w+)\s*\(/);
    if (pythonMatch?.[1])
        return pythonMatch[1];
    return null;
}
/**
 * Infer entity type from content
 */
function inferEntityType(result) {
    if (!result || !result.file_path)
        return 'unknown';
    const path = result.file_path.toLowerCase();
    if (path.includes('schema') || path.includes('drizzle'))
        return 'drizzle';
    if (path.includes('entities') || path.includes('entity'))
        return 'entity';
    if (path.includes('models') || path.includes('model'))
        return 'model';
    if (path.includes('domain'))
        return 'domain';
    return 'unknown';
}
/**
 * Truncate content to max lines
 */
function truncateContent(content, maxLines) {
    if (!content)
        return '';
    const lines = content.split('\n');
    if (lines.length <= maxLines)
        return content;
    return lines.slice(0, maxLines).join('\n') + '\n// ...';
}
//# sourceMappingURL=semantic.js.map