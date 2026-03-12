# yoyo — Instructions for Codex

## Testing note

- When doing BDD or release verification here, distinguish CLI behavior from MCP behavior.
- CLI tests validate command output and local binary flows.
- MCP tests validate JSON-RPC shape, tool registration, and first-contact agent guidance.
- Do not assume a passing CLI path proves the MCP surface is correct, or vice versa.
