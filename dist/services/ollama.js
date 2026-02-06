import { Ollama } from 'ollama';
let ollamaClient = null;
/**
 * Get or create Ollama client
 */
function getClient() {
    if (!ollamaClient) {
        ollamaClient = new Ollama({ host: 'http://localhost:11434' });
    }
    return ollamaClient;
}
/**
 * Check if Ollama is available and running
 */
export async function checkOllamaAvailable() {
    try {
        const client = getClient();
        await client.list();
        return true;
    }
    catch {
        return false;
    }
}
/**
 * Get list of available models
 */
export async function getAvailableModels() {
    try {
        const client = getClient();
        const { models } = await client.list();
        return models.map(m => m.name);
    }
    catch {
        return [];
    }
}
/**
 * Select the best available model
 * Preference: llama3.2 > codellama > mistral > qwen > any
 */
export async function selectBestModel(models) {
    const preferred = ['llama3.2', 'llama3.1', 'codellama', 'mistral', 'qwen', 'deepseek'];
    for (const name of preferred) {
        const found = models.find(m => m.toLowerCase().includes(name));
        if (found)
            return found;
    }
    return models[0] ?? null;
}
/**
 * Wrap a promise with a timeout
 */
function withTimeout(promise, ms, label) {
    return new Promise((resolve, reject) => {
        const timer = setTimeout(() => reject(new Error(`Ollama timeout after ${ms / 1000}s: ${label}`)), ms);
        promise.then((val) => { clearTimeout(timer); resolve(val); }, (err) => { clearTimeout(timer); reject(err); });
    });
}
/**
 * Generate text using Ollama
 */
export async function generate(prompt, options = {}) {
    const client = getClient();
    const { model = 'llama3.2', format, timeout = 60000 } = options;
    const response = await withTimeout(client.generate({ model, prompt, format, stream: false }), timeout, `generate(${model})`);
    return response.response;
}
/**
 * Generate JSON using Ollama
 */
export async function generateJSON(prompt, options = {}) {
    const response = await generate(prompt, { ...options, format: 'json' });
    try {
        return JSON.parse(response);
    }
    catch {
        // Try to extract JSON from response
        const match = response.match(/\{[\s\S]*\}/);
        if (match) {
            return JSON.parse(match[0]);
        }
        throw new Error('Failed to parse JSON from Ollama response');
    }
}
//# sourceMappingURL=ollama.js.map