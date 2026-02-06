/**
 * Hook: enforce-grepai
 *
 * Encourages use of grepai for semantic code search.
 * Triggers on Grep/Glob tools.
 *
 * Note: This is an advisory hook, not a blocker.
 */

export default {
  name: 'enforce-grepai',

  // Hook configuration
  hooks: {
    preToolCall: {
      tools: ['Grep', 'Glob'],
      handler: 'suggestGrepai'
    }
  },

  /**
   * Suggest using grepai for better semantic search
   */
  suggestGrepai(context) {
    const { toolName, parameters } = context;

    // Allow but remind
    return {
      allowed: true,
      message: `💡 SUGGESTION: Consider using grepai for semantic search.

grepai provides:
- Semantic understanding of code intent
- Better results for complex queries
- Call graph tracing

Example:
  grepai_search({ query: "your search" })
  grepai_trace_callers({ symbol: "FunctionName" })
`
    };
  }
};
