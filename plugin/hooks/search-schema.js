export default {
  "type": "object",
  "properties": {
    "request": {
      "type": "object",
      "properties": {
        "tool": {
          "type": "string",
          "enum": [
            "rg",
            "grep",
            "git",
            "Grep",
            "Glob",
            "Read",
            "Symbol",
            "Trace",
            "Structure",
            "References"
          ]
        },
        "input": {
          "type": "object"
        },
        "intent": {
          "type": "string",
          "description": "Specific local question, or 2–8 explicit '- question' bullets separated by newlines; keep constraints per question. Required for investigation or choose."
        },
        "purpose": {
          "type": "string",
          "enum": [
            "locate",
            "understand",
            "spec",
            "implement",
            "validate"
          ]
        },
        "choose": {
          "type": "boolean",
          "description": "Optional responsibility Choice after native triage. Default false."
        }
      },
      "required": [
        "tool",
        "input",
        "intent",
        "purpose"
      ],
      "additionalProperties": false,
      "oneOf": [
        {
          "properties": {
            "tool": {
              "const": "rg"
            },
            "input": {
              "type": "object",
              "properties": {
                "args": {
                  "type": "array",
                  "items": {
                    "type": "string"
                  },
                  "minItems": 1
                }
              },
              "required": [
                "args"
              ],
              "additionalProperties": false
            }
          },
          "required": [
            "tool",
            "input"
          ]
        },
        {
          "properties": {
            "tool": {
              "const": "grep"
            },
            "input": {
              "type": "object",
              "properties": {
                "args": {
                  "type": "array",
                  "items": {
                    "type": "string"
                  },
                  "minItems": 1
                }
              },
              "required": [
                "args"
              ],
              "additionalProperties": false
            }
          },
          "required": [
            "tool",
            "input"
          ]
        },
        {
          "properties": {
            "tool": {
              "const": "git"
            },
            "input": {
              "type": "object",
              "properties": {
                "args": {
                  "type": "array",
                  "items": {
                    "type": "string"
                  },
                  "minItems": 1
                }
              },
              "required": [
                "args"
              ],
              "additionalProperties": false
            }
          },
          "required": [
            "tool",
            "input"
          ]
        },
        {
          "properties": {
            "tool": {
              "const": "Grep"
            },
            "input": {
              "type": "object",
              "properties": {
                "pattern": {
                  "type": "string"
                },
                "path": {
                  "type": "string"
                },
                "glob": {
                  "type": "string"
                },
                "type": {
                  "type": "string"
                },
                "output_mode": {
                  "type": "string",
                  "enum": [
                    "content",
                    "files_with_matches",
                    "count"
                  ]
                },
                "-A": {
                  "type": "integer",
                  "minimum": 0
                },
                "-B": {
                  "type": "integer",
                  "minimum": 0
                },
                "-C": {
                  "type": "integer",
                  "minimum": 0
                },
                "context": {
                  "type": "integer",
                  "minimum": 0
                },
                "head_limit": {
                  "type": "integer",
                  "minimum": 0
                },
                "offset": {
                  "type": "integer",
                  "minimum": 0
                },
                "-n": {
                  "type": "boolean"
                },
                "-i": {
                  "type": "boolean"
                },
                "multiline": {
                  "type": "boolean"
                }
              },
              "required": [
                "pattern"
              ],
              "additionalProperties": false
            }
          },
          "required": [
            "tool",
            "input"
          ]
        },
        {
          "properties": {
            "tool": {
              "const": "Glob"
            },
            "input": {
              "type": "object",
              "properties": {
                "pattern": {
                  "type": "string"
                },
                "path": {
                  "type": "string"
                }
              },
              "required": [
                "pattern"
              ],
              "additionalProperties": false
            }
          },
          "required": [
            "tool",
            "input"
          ]
        },
        {
          "properties": {
            "tool": {
              "const": "Read"
            },
            "input": {
              "type": "object",
              "properties": {
                "file_path": {
                  "type": "string"
                },
                "offset": {
                  "type": "integer",
                  "minimum": 1
                },
                "limit": {
                  "type": "integer",
                  "minimum": 0
                }
              },
              "required": [
                "file_path"
              ],
              "additionalProperties": false
            }
          },
          "required": [
            "tool",
            "input"
          ]
        },
        {
          "properties": {
            "tool": {
              "const": "Symbol"
            },
            "input": {
              "type": "object",
              "properties": {
                "file_path": {
                  "type": "string"
                },
                "symbol": {
                  "type": "string"
                }
              },
              "required": [
                "file_path",
                "symbol"
              ],
              "additionalProperties": false
            }
          },
          "required": [
            "tool",
            "input"
          ]
        },
        {
          "properties": {
            "tool": {
              "const": "Trace"
            },
            "input": {
              "type": "object",
              "properties": {
                "file_path": {
                  "type": "string"
                },
                "symbol": {
                  "type": "string"
                },
                "direction": {
                  "type": "string",
                  "enum": [
                    "outgoing",
                    "callers",
                    "both"
                  ]
                },
                "depth": {
                  "type": "integer",
                  "minimum": 0
                },
                "limit": {
                  "type": "integer",
                  "minimum": 1
                },
                "target": {
                  "type": "string"
                }
              },
              "required": [
                "file_path",
                "symbol"
              ],
              "additionalProperties": false
            }
          },
          "required": [
            "tool",
            "input"
          ]
        },
        {
          "properties": {
            "tool": {
              "const": "Structure"
            },
            "input": {
              "type": "object",
              "properties": {
                "file_path": {
                  "type": "string"
                },
                "query": {
                  "type": "string"
                }
              },
              "required": [
                "file_path",
                "query"
              ],
              "additionalProperties": false
            }
          },
          "required": [
            "tool",
            "input"
          ]
        },
        {
          "properties": {
            "tool": {
              "const": "References"
            },
            "input": {
              "type": "object",
              "properties": {
                "file_path": {
                  "type": "string"
                },
                "line": {
                  "type": "integer",
                  "minimum": 1
                },
                "column": {
                  "type": "integer",
                  "minimum": 0
                },
                "relation": {
                  "type": "string",
                  "enum": [
                    "definitions",
                    "references",
                    "implementations"
                  ]
                },
                "limit": {
                  "type": "integer",
                  "minimum": 1
                }
              },
              "required": [
                "file_path",
                "line",
                "column"
              ],
              "additionalProperties": false
            }
          },
          "required": [
            "tool",
            "input"
          ]
        }
      ]
    }
  },
  "required": [
    "request"
  ],
  "additionalProperties": false
};
