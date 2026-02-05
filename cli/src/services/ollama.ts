import { Ollama } from 'ollama';
import type { OllamaOptions } from '../types.js';

let ollamaClient: Ollama | null = null;

/**
 * Get or create Ollama client
 */
function getClient(): Ollama {
  if (!ollamaClient) {
    ollamaClient = new Ollama({ host: 'http://localhost:11434' });
  }
  return ollamaClient;
}

/**
 * Check if Ollama is available and running
 */
export async function checkOllamaAvailable(): Promise<boolean> {
  try {
    const client = getClient();
    await client.list();
    return true;
  } catch {
    return false;
  }
}

/**
 * Get list of available models
 */
export async function getAvailableModels(): Promise<string[]> {
  try {
    const client = getClient();
    const { models } = await client.list();
    return models.map(m => m.name);
  } catch {
    return [];
  }
}

/**
 * Select the best available model
 * Preference: llama3.2 > codellama > mistral > qwen > any
 */
export async function selectBestModel(models: string[]): Promise<string | null> {
  const preferred = ['llama3.2', 'llama3.1', 'codellama', 'mistral', 'qwen', 'deepseek'];

  for (const name of preferred) {
    const found = models.find(m => m.toLowerCase().includes(name));
    if (found) return found;
  }

  return models[0] ?? null;
}

/**
 * Wrap a promise with a timeout
 */
function withTimeout<T>(promise: Promise<T>, ms: number, label: string): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error(`Ollama timeout after ${ms / 1000}s: ${label}`)), ms);
    promise.then(
      (val) => { clearTimeout(timer); resolve(val); },
      (err) => { clearTimeout(timer); reject(err); }
    );
  });
}

/**
 * Generate text using Ollama
 */
export async function generate(prompt: string, options: OllamaOptions = {}): Promise<string> {
  const client = getClient();
  const { model = 'llama3.2', format, timeout = 60000 } = options;

  const response = await withTimeout(
    client.generate({ model, prompt, format, stream: false }),
    timeout,
    `generate(${model})`
  );

  return response.response;
}

/**
 * Generate JSON using Ollama
 */
export async function generateJSON<T = unknown>(prompt: string, options: OllamaOptions = {}): Promise<T> {
  const response = await generate(prompt, { ...options, format: 'json' });

  try {
    return JSON.parse(response) as T;
  } catch {
    // Try to extract JSON from response
    const match = response.match(/\{[\s\S]*\}/);
    if (match) {
      return JSON.parse(match[0]) as T;
    }
    throw new Error('Failed to parse JSON from Ollama response');
  }
}
