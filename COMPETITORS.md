# yoyo — Competitive Landscape

Last updated: 2026-03-08

---

## MCP Servers — Direct Competitors

### ast-grep MCP
- **What:** Structural code search via AST pattern matching. 4 tools: `dump_syntax_tree`, `test_match_code_rule`, `find_code`, `find_code_by_rule`.
- **Approach:** Search-only, no persistent index, no write operations.
- **Strength:** Multi-language structural search (25+ langs via tree-sitter).
- **Gap vs yoyo:** No bake/shake lifecycle, no edit/patch, read-only.
- **Links:** [GitHub](https://github.com/ast-grep/ast-grep-mcp)

### Code Pathfinder MCP
- **What:** Python-focused. Call graphs, import resolution, dataflow tracking, 5-pass AST static analysis, 6 tools.
- **Approach:** Index-based, Python-only, AGPL-3.0.
- **Strength:** Deep Python call graph with bidirectional traversal and dataflow.
- **Gap vs yoyo:** Language-locked, no write tools, no LLM instruction bootstrap.
- **Links:** [codepathfinder.dev](https://codepathfinder.dev/mcp)

### code-graph-mcp (Rust)
- **What:** Code intelligence across 25+ languages. Graph + FAISS vector indexing. 9 analysis tools.
- **Approach:** RocksDB graph + FAISS embeddings. Heavyweight infra.
- **Strength:** Semantic similarity search via embeddings, fast repeated queries via LRU cache.
- **Gap vs yoyo:** No write tools, no CLI, embedding model dependency, complex ops footprint.
- **Links:** [GitHub](https://github.com/entrepeneur4lyf/code-graph-mcp)

### CodeGraphContext
- **What:** Indexes codebases into Neo4j. Function call relationships, class hierarchies, dead code detection, real-time file monitoring.
- **Approach:** External graph DB (Neo4j) required. File-watch incremental updates.
- **Strength:** Full Cypher query power. Dead code detection.
- **Gap vs yoyo:** Requires running Neo4j — ops burden. No write tools. Not self-contained.
- **Links:** [GitHub](https://github.com/CodeGraphContext/CodeGraphContext)

### AST MCP Server (angrysky56)
- **What:** AST + Abstract Semantic Graph (ASG) analysis. Tools: `parse_to_ast`, `generate_asg`, `analyze_code`, incremental parsing.
- **Approach:** In-memory parse-and-cache.
- **Strength:** ASG gives semantic relationships beyond raw AST.
- **Gap vs yoyo:** Experimental, no write tools, no CLI, no LLM instruction layer.
- **Links:** [GitHub](https://github.com/angrysky56/ast-mcp-server)

### Joern MCP
- **What:** Exposes Joern's Code Property Graph (CPG). Static analysis, vulnerability detection, taint tracking.
- **Approach:** Security niche. Requires Joern runtime (JVM, heavy).
- **Strength:** Best-in-class vuln analysis and taint flow for security research.
- **Gap vs yoyo:** Security/audit niche only. Heavy dependency chain.

### LSP-MCP Bridges (mcp-language-server, lsp-mcp, mcpls)
- **What:** Bridge LSP to MCP. Expose go-to-definition, find-references, hover, diagnostics, rename, completions.
- **Approach:** Delegate to running language servers (rust-analyzer, pyright, tsserver, etc.).
- **Strength:** IDE-fidelity — type inference, semantic rename, cross-reference analysis.
- **Gap vs yoyo:** Runtime dependency on per-language LSP. No persistent index. No write/scaffold tools.
- **Links:** [mcp-language-server](https://github.com/isaacphi/mcp-language-server) | [lsp-mcp](https://github.com/Tritlo/lsp-mcp)

---

## Integrated Agent Platforms — Broader Competitors

### Cursor
- **What:** Forked VS Code with proprietary embedding-based codebase indexing. Tab-completion with multi-file awareness.
- **Strength:** Best-in-class UX for AI-assisted editing. Entire codebase in context.
- **Gap vs yoyo:** Closed, editor-bound, not composable. End-product, not infrastructure.

### GitHub Copilot / Copilot Workspace
- **What:** Pattern-completion from pre-trained model. Workspace adds multi-file planning/editing.
- **Strength:** Deep IDE integration, GitHub repo context.
- **Gap vs yoyo:** No structural code understanding. Completion-first, not analysis-first.

### Sourcegraph Cody / Amp CLI
- **What:** "Search-first" indexing across entire repos. MCP server for org-wide search. Free/pro discontinued July 2025; successor is Amp CLI.
- **Strength:** Cross-repo search at org scale. Strong for monorepos.
- **Gap vs yoyo:** Cloud-dependent, enterprise-scale, read-only context retrieval, no write tools.

### Aider
- **What:** CLI coding agent. Repo-map (ctags-based) gives LLMs structural context. LLM edits via structured diffs. Git-native, auto-commits.
- **Strength:** Best-in-class multi-file edit workflow. Mature, widely used.
- **Gap vs yoyo:** Repo-map is symbol-level (ctags), not AST-deep. Coding agent, not MCP infrastructure. No composability.

### Continue.dev
- **What:** Open-source VS Code/JetBrains plugin. Chat, Plan, Agent modes. Configurable model + context providers.
- **Strength:** Open, extensible, IDE-embedded, supports local models.
- **Gap vs yoyo:** UI/IDE product. Context is file-snippet based, not index-based.

---

## Differentiation Summary

| Dimension | Most competitors | yoyo |
|---|---|---|
| Persistent index | None or external DB required | `bake` + `shake`, local |
| Write tools | Absent | `patch`, `graph_add`, `graph_create`, `multi_patch` |
| LLM onboarding | None | `llm_instructions` bootstrap |
| CLI + MCP unified | Rarely | Same engine, two adapters |
| Self-contained binary | No (runtime + deps) | Single Rust binary |
| Language scope | Python-only or needs LSP | Multi-language via tree-sitter |
| Security analysis | Joern specializes here | Not the goal |
| Org-scale cross-repo | Sourcegraph specializes here | Not the goal |

---

## Strategic read

- **Nobody else has write tools** at the MCP layer. `patch`, `multi_patch`, `graph_create` are effectively unchallenged.
- **The persistent local index** (bake/shake) is yoyo's deepest moat. Competitors parse on-demand or require external DBs.
- **LSP bridges are the most credible threat** for navigation-heavy use cases — but they can't scaffold, can't patch, can't do health/blast_radius analysis.
- **Aider is the most mature agent workflow tool** but operates at a different layer — it's a peer, not a replacement.
- **Cursor is the dominant end-product** but it's closed. yoyo is infrastructure that works *inside* any agent, including Cursor alternatives.
