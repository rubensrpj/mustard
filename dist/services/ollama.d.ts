import type { OllamaOptions } from '../types.js';
/**
 * Check if Ollama is available and running
 */
export declare function checkOllamaAvailable(): Promise<boolean>;
/**
 * Get list of available models
 */
export declare function getAvailableModels(): Promise<string[]>;
/**
 * Select the best available model
 * Preference: llama3.2 > codellama > mistral > qwen > any
 */
export declare function selectBestModel(models: string[]): Promise<string | null>;
/**
 * Generate text using Ollama
 */
export declare function generate(prompt: string, options?: OllamaOptions): Promise<string>;
/**
 * Generate JSON using Ollama
 */
export declare function generateJSON<T = unknown>(prompt: string, options?: OllamaOptions): Promise<T>;
