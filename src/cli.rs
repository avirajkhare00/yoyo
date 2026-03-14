use clap::{Args, Subcommand, ValueEnum};

/// High-level yoyo commands exposed to humans.
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Prime directive and usage instructions for yoyo.
    Boot(BootArgs),
    /// Task-oriented guidance for choosing the right yoyo tools.
    Guide(GuideArgs),
    /// Full reference catalog: workflows, decision map, antipatterns, metapatterns.
    Recipes(RecipesArgs),
    /// Repository overview similar to Shake.readme.
    Stats(StatsArgs),
    /// Build and persist an index under the project root.
    Index(IndexArgs),
    /// Detailed lookup of a function symbol from the bake index.
    Symbol(SymbolArgs),
    /// Inspect a symbol, file outline, or line range from one entrypoint.
    Inspect(InspectArgs),
    /// List all detected API endpoints from the bake index.
    Routes(RoutesArgs),
    /// Understand symbol or endpoint impact from one entrypoint.
    Impact(ImpactArgs),
    /// Judge the ownership layer, candidate symbols, invariants, and risks before editing.
    JudgeChange(JudgeChangeArgs),
    /// Vertical slice: endpoint → handler → call chain in one call.
    Flow(FlowArgs),
    /// Read a specific line range of a file.
    Read(ReadArgs),
    /// Per-file function overview from the bake index.
    Outline(OutlineArgs),
    /// Text-based search over TS/JS source files.
    Search(SearchArgs),
    /// Deep-dive summary of a package/module directory.
    Module(ModuleArgs),
    /// Project structure map and placement hints.
    Map(MapArgs),
    /// Suggest where to place a new function.
    Where(WhereArgs),
    /// Find documentation/config files.
    Docs(DocsArgs),
    /// Task-shaped write entrypoint over edit, rename, move, delete, create, add, and bulk-edit.
    Change(ChangeArgs),
    /// Apply a patch by symbol name or by file/line range.
    Edit(EditArgs),
    /// Turn a failed guarded write into a bounded retry plan.
    RetryPlan(RetryPlanArgs),
    /// Analyse the blast radius of a symbol (transitive callers + affected files).
    Callers(CallersArgs),
    /// Rename a symbol everywhere (definition + all call sites) atomically.
    Rename(RenameArgs),
    /// Create a new file with an initial function scaffold.
    Create(CreateArgs),
    /// Insert a new function scaffold into a file.
    Add(AddArgs),
    /// Move a function from one file to another.
    /// Move a function from one file to another.
    Move(MoveArgs),
    /// Trace a function's call chain downward to external boundaries.
    Calls(CallsArgs),
    /// Audit dead code, large functions, and duplicate hints.
    Health(HealthArgs),
    /// Remove a function from a file by name.
    Delete(DeleteArgs),
    /// Search for functions by natural-language intent (local TF-IDF, no external deps).
    Ask(AskArgs),
    /// Execute a Rhai script with yoyo tools available as functions.
    Script(ScriptArgs),
    /// Update yoyo to the latest release.
    Update(UpdateArgs),
}

#[derive(Clone, Debug, ValueEnum)]
pub enum OutputView {
    Compact,
    Full,
    Raw,
}

impl OutputView {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Compact => "compact",
            Self::Full => "full",
            Self::Raw => "raw",
        }
    }
}

#[derive(Args, Debug)]
pub struct BootArgs {
    /// Optional path to the project directory to analyze.
    #[arg(long)]
    pub path: Option<String>,
}

#[derive(Args, Debug)]
pub struct GuideArgs {
    /// Tool or task topic, e.g. inspect, safe delete, trace request.
    #[arg(long)]
    pub topic: String,
}

#[derive(Args, Debug)]
pub struct RecipesArgs {
    /// Optional path to the project directory (unused; kept for API symmetry).
    #[arg(long)]
    pub path: Option<String>,

    /// Response view: compact for paged summaries, raw/full for the full catalog.
    #[arg(long, value_enum, default_value = "raw")]
    pub view: OutputView,

    /// Items per section when using --view compact (default 3).
    #[arg(long)]
    pub limit: Option<usize>,

    /// Section cursor in the form <section>:<offset>, returned by a previous compact response.
    #[arg(long)]
    pub cursor: Option<String>,

    /// Natural-language query: return top matching workflows, decisions, and antipatterns.
    #[arg(long)]
    pub query: Option<String>,
}

#[derive(Args, Debug)]
pub struct StatsArgs {
    /// Optional path to the project directory to analyze.
    #[arg(long)]
    pub path: Option<String>,
}

#[derive(Args, Debug)]
pub struct IndexArgs {
    /// Optional path to the project directory to analyze.
    #[arg(long)]
    pub path: Option<String>,
}

#[derive(Args, Debug)]
pub struct SymbolArgs {
    /// Optional path to the project directory to analyze.
    #[arg(long)]
    pub path: Option<String>,

    /// Symbol (function) name to look up.
    #[arg(long)]
    pub name: String,

    /// Include function body (source) inline in each match.
    #[arg(long, default_value_t = false)]
    pub include_source: bool,

    /// Optional file path substring to narrow results (e.g. 'tcp_core' or 'routes/user').
    #[arg(long)]
    pub file: Option<String>,

    /// Maximum number of matches to return (default 20).
    #[arg(long)]
    pub limit: Option<usize>,

    /// Also search installed toolchain stdlibs (Zig/Go/Rust). Matches are tagged is_stdlib: true.
    #[arg(long, default_value_t = false)]
    pub stdlib: bool,
}

#[derive(Args, Debug)]
pub struct InspectArgs {
    /// Optional path to the project directory to analyze.
    #[arg(long)]
    pub path: Option<String>,

    /// Function name for symbol mode.
    #[arg(long)]
    pub name: Option<String>,

    /// File path for file mode or line-range mode.
    #[arg(long)]
    pub file: Option<String>,

    /// 1-based start line for line-range mode.
    #[arg(long)]
    pub start_line: Option<u32>,

    /// 1-based end line for line-range mode.
    #[arg(long)]
    pub end_line: Option<u32>,

    /// Include function body in symbol mode.
    #[arg(long, default_value_t = false)]
    pub include_source: bool,

    /// Return only declaration/signature text for symbol matches instead of full bodies.
    #[arg(long, default_value_t = false)]
    pub signature_only: bool,

    /// Read type surfaces directly: declaration, fields, methods, and impl metadata.
    #[arg(long, default_value_t = false)]
    pub type_only: bool,

    /// Include summaries in file mode.
    #[arg(long, default_value_t = true)]
    pub include_summaries: bool,

    /// File structure depth in file mode: 1 (top-level), 2 (types + members), or all.
    #[arg(long)]
    pub depth: Option<String>,

    /// Maximum number of symbol matches.
    #[arg(long)]
    pub limit: Option<usize>,

    /// Include stdlib matches in symbol mode.
    #[arg(long, default_value_t = false)]
    pub stdlib: bool,
}

#[derive(Args, Debug)]
pub struct ChangeArgs {
    /// Optional path to the project directory to analyze.
    #[arg(long)]
    pub path: Option<String>,

    /// Write action: edit | bulk_edit | rename | move | delete | create | add.
    #[arg(long)]
    pub action: String,

    /// Symbol name for edit/rename/move/delete/add.
    #[arg(long)]
    pub name: Option<String>,

    /// File path for edit/create/add.
    #[arg(long)]
    pub file: Option<String>,

    /// 1-based start line for line-range edit.
    #[arg(long)]
    pub start_line: Option<u32>,

    /// 1-based end line for line-range edit.
    #[arg(long)]
    pub end_line: Option<u32>,

    /// Replacement content for edit.
    #[arg(long)]
    pub new_content: Option<String>,

    /// Exact content-match source for edit.
    #[arg(long)]
    pub old_string: Option<String>,

    /// Content-match replacement for edit or rename target.
    #[arg(long)]
    pub new_string: Option<String>,

    /// Rename target.
    #[arg(long)]
    pub new_name: Option<String>,

    /// Destination file for move.
    #[arg(long)]
    pub to_file: Option<String>,

    /// Allow delete even with callers.
    #[arg(long, default_value_t = false)]
    pub force: bool,

    /// Function name for create.
    #[arg(long)]
    pub function_name: Option<String>,

    /// Scaffold type for add.
    #[arg(long)]
    pub entity_type: Option<String>,

    /// Insert add scaffold after an existing symbol.
    #[arg(long)]
    pub after_symbol: Option<String>,

    /// Optional language override for create/add.
    #[arg(long)]
    pub language: Option<String>,

    /// 0-based symbol disambiguation for edit by name.
    #[arg(long)]
    pub match_index: Option<usize>,

    /// JSON array of edits for bulk_edit action.
    #[arg(long)]
    pub edits_json: Option<String>,

    /// Path to a JSON file containing bulk_edit edits.
    #[arg(long)]
    pub edits_file: Option<String>,

    /// JSON array of typed params for create/add, e.g. [{"name":"x","type_str":"i32"}].
    #[arg(long)]
    pub params_json: Option<String>,

    /// Optional return type for create/add.
    #[arg(long)]
    pub returns: Option<String>,

    /// Optional receiver/owner type for add.
    #[arg(long)]
    pub on: Option<String>,
}

#[derive(Args, Debug)]
pub struct RoutesArgs {
    /// Optional path to the project directory to analyze.
    #[arg(long)]
    pub path: Option<String>,

    /// Optional path or handler substring to narrow endpoint results.
    #[arg(long)]
    pub query: Option<String>,

    /// Optional HTTP method filter.
    #[arg(long)]
    pub method: Option<String>,

    /// Optional workspace/package/slice hint for monorepos, e.g. backend or web.
    #[arg(long)]
    pub scope: Option<String>,

    /// Maximum number of endpoints to return.
    #[arg(long)]
    pub limit: Option<usize>,
}

#[derive(Args, Debug)]
pub struct ImpactArgs {
    /// Optional path to the project directory.
    #[arg(long)]
    pub path: Option<String>,

    /// Function name for symbol-impact mode.
    #[arg(long)]
    pub symbol: Option<String>,

    /// URL path substring for endpoint-impact mode.
    #[arg(long)]
    pub endpoint: Option<String>,

    /// Optional HTTP method filter for endpoint mode.
    #[arg(long)]
    pub method: Option<String>,

    /// Max caller/call-chain depth.
    #[arg(long)]
    pub depth: Option<usize>,

    /// Include handler source inline in endpoint mode.
    #[arg(long, default_value_t = false)]
    pub include_source: bool,

    /// Optional workspace/package/slice hint for monorepos, e.g. backend or web.
    #[arg(long)]
    pub scope: Option<String>,
}

#[derive(Args, Debug)]
pub struct JudgeChangeArgs {
    /// Optional path to the project directory.
    #[arg(long)]
    pub path: Option<String>,

    /// Engineering question, issue text, or failing-test summary.
    #[arg(long)]
    pub query: String,

    /// Optional symbol hint to bias the judgment toward a known name.
    #[arg(long)]
    pub symbol: Option<String>,

    /// Optional file path substring to restrict the search surface.
    #[arg(long)]
    pub file: Option<String>,

    /// Maximum number of candidate symbols to return (default 3, max 5).
    #[arg(long)]
    pub limit: Option<usize>,

    /// Optional workspace/package/slice hint for monorepos, e.g. backend or web.
    #[arg(long)]
    pub scope: Option<String>,
}

#[derive(Args, Debug)]
pub struct FlowArgs {
    /// Optional path to the project directory.
    #[arg(long)]
    pub path: Option<String>,

    /// URL path substring to match (e.g. '/users' or '/api/login').
    #[arg(long)]
    pub endpoint: String,

    /// Optional HTTP method filter (GET, POST, PUT, DELETE, PATCH).
    #[arg(long)]
    pub method: Option<String>,

    /// Max call chain depth (default 5).
    #[arg(long)]
    pub depth: Option<usize>,

    /// Include the handler function source inline.
    #[arg(long)]
    pub include_source: bool,

    /// Optional workspace/package/slice hint for monorepos, e.g. backend or web.
    #[arg(long)]
    pub scope: Option<String>,
}

#[derive(Args, Debug)]
pub struct ReadArgs {
    /// Optional path to the project directory to analyze.
    #[arg(long)]
    pub path: Option<String>,

    /// File path relative to the project root.
    #[arg(long)]
    pub file: String,

    /// 1-based start line (inclusive).
    #[arg(long)]
    pub start: u32,

    /// 1-based end line (inclusive).
    #[arg(long)]
    pub end: u32,
}

#[derive(Args, Debug)]
pub struct OutlineArgs {
    /// Optional path to the project directory to analyze.
    #[arg(long)]
    pub path: Option<String>,

    /// File path relative to the project root.
    #[arg(long)]
    pub file: String,

    /// Whether to include summaries (currently a no-op placeholder).
    #[arg(long, default_value_t = true)]
    pub include_summaries: bool,

    /// File structure depth: 1 (top-level), 2 (types + members), or all.
    #[arg(long)]
    pub depth: Option<String>,
}

#[derive(Args, Debug)]
pub struct SearchArgs {
    /// Optional path to the project directory to analyze.
    #[arg(long)]
    pub path: Option<String>,

    /// Search query text.
    #[arg(long)]
    pub query: String,

    /// Search context: all | strings | comments | identifiers.
    #[arg(long, default_value = "all")]
    pub context: String,

    /// Pattern: all | call | assign | return.
    #[arg(long, default_value = "all")]
    pub pattern: String,

    /// Whether to exclude likely test files.
    #[arg(long, default_value_t = true)]
    pub exclude_tests: bool,

    /// Optional file path substring to restrict search scope.
    #[arg(long)]
    pub file: Option<String>,

    /// Maximum number of matches to return (default 200).
    #[arg(long)]
    pub limit: Option<usize>,
}

#[derive(Args, Debug)]
pub struct ModuleArgs {
    /// Optional path to the project directory to analyze.
    #[arg(long)]
    pub path: Option<String>,

    /// Package/module name or directory substring. Omit to return all packages.
    #[arg(long)]
    pub package: Option<String>,
}

#[derive(Args, Debug)]
pub struct MapArgs {
    /// Optional path to the project directory to analyze.
    #[arg(long)]
    pub path: Option<String>,

    /// Intent description, e.g. "user handler" or "auth service".
    #[arg(long)]
    pub intent: Option<String>,

    /// Max directories to return (default 100).
    #[arg(long)]
    pub limit: Option<usize>,
}

#[derive(Args, Debug)]
pub struct WhereArgs {
    /// Optional path to the project directory to analyze.
    #[arg(long)]
    pub path: Option<String>,

    /// Name of the function to add.
    #[arg(long)]
    pub function_name: String,

    /// Function type: handler | service | repository | model | util | test.
    #[arg(long)]
    pub function_type: String,

    /// Existing related symbol or substring (optional).
    #[arg(long)]
    pub related_to: Option<String>,
}

#[derive(Args, Debug)]
pub struct DocsArgs {
    /// Optional path to the project directory to analyze.
    #[arg(long)]
    pub path: Option<String>,

    /// Documentation type: readme | env | config | docker | all. Defaults to all.
    #[arg(long)]
    pub doc_type: Option<String>,

    /// Maximum number of results to return (default 50).
    #[arg(long, default_value_t = 50)]
    pub limit: usize,
}

#[derive(Args, Debug)]
pub struct EditArgs {
    /// Optional path to the project directory.
    #[arg(long)]
    pub path: Option<String>,

    /// Patch by symbol name (resolves file and line range from bake index).
    #[arg(long)]
    pub symbol: Option<String>,

    /// When multiple symbols match --symbol, use this 0-based index (default 0).
    #[arg(long)]
    pub match_index: Option<usize>,

    /// File path relative to the project root (for range-based patch; use with --start, --end).
    #[arg(long)]
    pub file: Option<String>,

    /// 1-based start line (inclusive). Required for range-based patch.
    #[arg(long)]
    pub start: Option<u32>,

    /// 1-based end line (inclusive). Required for range-based patch.
    #[arg(long)]
    pub end: Option<u32>,

    /// Replacement content for the patched range.
    #[arg(long)]
    pub new_content: String,
}

#[derive(Args, Debug)]
pub struct RetryPlanArgs {
    /// Optional project directory. Falls back to project_root embedded in the guard_failure payload.
    #[arg(long)]
    pub path: Option<String>,

    /// Failed write output containing a `guard_failure: {...}` line, or a raw guard_failure JSON object.
    #[arg(long)]
    pub text: Option<String>,

    /// Path to a file containing failed write output or a raw guard_failure JSON object.
    #[arg(long)]
    pub text_file: Option<String>,

    /// Maximum retry attempts the caller should allow before stopping.
    #[arg(long, default_value_t = 2)]
    pub max_retries: usize,

    /// Context lines to include above and below the failing range.
    #[arg(long, default_value_t = 3)]
    pub context_lines: u32,
}

#[derive(Args, Debug)]
pub struct CallersArgs {
    /// Optional path to the project directory to analyze.
    #[arg(long)]
    pub path: Option<String>,

    /// Function name to analyse (exact match on the callee name).
    #[arg(long)]
    pub symbol: String,

    /// Maximum call-graph depth to traverse (default 2).
    #[arg(long)]
    pub depth: Option<usize>,
}

#[derive(Args, Debug)]
pub struct RenameArgs {
    /// Optional path to the project directory.
    #[arg(long)]
    pub path: Option<String>,

    /// Current identifier name to rename.
    #[arg(long)]
    pub name: String,

    /// New identifier name.
    #[arg(long)]
    pub new_name: String,
}

#[derive(Args, Debug)]
pub struct AddArgs {
    /// Optional path to the project directory.
    #[arg(long)]
    pub path: Option<String>,

    /// Scaffold type: fn | function | def | func.
    #[arg(long)]
    pub entity_type: String,

    /// Name for the new function/entity.
    #[arg(long)]
    pub name: String,

    /// File path relative to project root.
    #[arg(long)]
    pub file: String,

    /// Insert after this existing symbol (name or substring).
    #[arg(long)]
    pub after_symbol: Option<String>,

    /// Override language detection (rust | typescript | python | go).
    #[arg(long)]
    pub language: Option<String>,
}

#[derive(Args, Debug)]
pub struct CreateArgs {
    /// Optional path to the project directory.
    #[arg(long)]
    pub path: Option<String>,

    /// File path relative to project root (e.g. src/engine/foo.rs).
    #[arg(long)]
    pub file: String,

    /// Name for the initial scaffolded function.
    #[arg(long)]
    pub function_name: String,

    /// Override language detection (rust | typescript | python | go).
    #[arg(long)]
    pub language: Option<String>,
}

#[derive(Args, Debug)]
pub struct MoveArgs {
    /// Optional path to the project directory.
    #[arg(long)]
    pub path: Option<String>,

    /// Exact function name to move.
    #[arg(long)]
    pub name: String,

    /// Destination file path relative to project root.
    #[arg(long)]
    pub to_file: String,
}

#[derive(Args, Debug)]
pub struct AskArgs {
    /// Optional path to the project directory.
    #[arg(long)]
    pub path: Option<String>,

    /// Natural-language description of what you're looking for.
    #[arg(long)]
    pub query: String,

    /// Max results to return (default 10, max 50).
    #[arg(long)]
    pub limit: Option<usize>,

    /// Optional file path substring to restrict scope.
    #[arg(long)]
    pub file: Option<String>,

    /// Optional workspace/package/slice hint for monorepos, e.g. backend or web.
    #[arg(long)]
    pub scope: Option<String>,
}

pub async fn run(command: Option<Command>) -> anyhow::Result<()> {
    match command {
        Some(Command::Boot(args)) => run_boot(args).await?,
        Some(Command::Guide(args)) => run_guide(args).await?,
        Some(Command::Recipes(args)) => run_recipes(args).await?,
        Some(Command::Stats(args)) => run_stats(args).await?,
        Some(Command::Index(args)) => run_index(args).await?,
        Some(Command::Symbol(args)) => run_symbol(args).await?,
        Some(Command::Inspect(args)) => run_inspect(args).await?,
        Some(Command::Routes(args)) => run_routes(args).await?,
        Some(Command::Impact(args)) => run_impact(args).await?,
        Some(Command::JudgeChange(args)) => run_judge_change(args).await?,
        Some(Command::Flow(args)) => run_flow(args).await?,
        Some(Command::Read(args)) => run_read(args).await?,
        Some(Command::Outline(args)) => run_outline(args).await?,
        Some(Command::Search(args)) => run_search(args).await?,
        Some(Command::Module(args)) => run_module(args).await?,
        Some(Command::Map(args)) => run_map(args).await?,
        Some(Command::Where(args)) => run_where(args).await?,
        Some(Command::Docs(args)) => run_docs(args).await?,
        Some(Command::Change(args)) => run_change(args).await?,
        Some(Command::Edit(args)) => run_edit(args).await?,
        Some(Command::RetryPlan(args)) => run_retry_plan(args).await?,
        Some(Command::Callers(args)) => run_callers(args).await?,
        Some(Command::Rename(args)) => run_rename(args).await?,
        Some(Command::Create(args)) => run_create(args).await?,
        Some(Command::Add(args)) => run_add(args).await?,
        Some(Command::Move(args)) => run_move(args).await?,
        Some(Command::Calls(args)) => run_calls(args).await?,
        Some(Command::Health(args)) => run_health(args).await?,
        Some(Command::Delete(args)) => run_delete(args).await?,
        Some(Command::Ask(args)) => run_ask(args).await?,
        Some(Command::Script(args)) => run_script(args).await?,
        Some(Command::Update(args)) => run_update(args).await?,
        None => {
            let exe = std::env::current_exe()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| "/usr/local/bin/yoyo".to_string());
            println!(
                "yoyo v{} — code intelligence MCP server",
                env!("CARGO_PKG_VERSION")
            );
            println!();
            println!("Getting started:");
            println!();
            println!("  1. Index your project");
            println!("     yoyo index --path /path/to/your/project");
            println!();
            println!("  2. Connect to Claude Code");
            println!("     Add to ~/.claude/settings.json:");
            println!();
            println!("     {{");
            println!("       \"mcpServers\": {{");
            println!("         \"yoyo\": {{");
            println!("           \"type\": \"stdio\",");
            println!("           \"command\": \"{exe}\",");
            println!("           \"args\": [\"--mcp-server\"]");
            println!("         }}");
            println!("       }}");
            println!("     }}");
            println!();
            println!("  3. Add the hook (makes Claude prefer yoyo tools over grep/cat)");
            println!("     In your project: .claude/settings.local.json");
            println!();
            println!("     {{");
            println!("       \"hooks\": {{");
            println!("         \"UserPromptSubmit\": [{{");
            println!("           \"hooks\": [{{");
            println!("             \"type\": \"command\",");
            println!("             \"command\": \"echo '[yoyo] Use supersearch not grep. Use symbol+include_source not cat.'\"");
            println!("           }}]");
            println!("         }}]");
            println!("       }}");
            println!("     }}");
            println!();
            println!("  4. Restart Claude Code, then start a session");
            println!("     Claude calls llm_instructions automatically on first contact.");
            println!();
            println!("Keep yoyo current:");
            println!("  yoyo update          self-update binary");
            println!("  brew upgrade yoyo    if installed via Homebrew");
            println!();
            println!("All commands:  yoyo --help");
            println!("Full docs:    https://github.com/avirajkhare00/yoyo");
        }
    }
    Ok(())
}

async fn run_boot(args: BootArgs) -> anyhow::Result<()> {
    let json = crate::engine::llm_instructions(args.path)?;
    println!("{json}");
    Ok(())
}

async fn run_guide(args: GuideArgs) -> anyhow::Result<()> {
    let json = crate::engine::tool_help(args.topic)?;
    println!("{json}");
    Ok(())
}

async fn run_recipes(args: RecipesArgs) -> anyhow::Result<()> {
    let json = crate::engine::llm_workflows(
        args.path,
        Some(args.view.as_str().to_string()),
        args.limit,
        args.cursor,
        args.query,
    )?;
    println!("{json}");
    Ok(())
}

async fn run_stats(args: StatsArgs) -> anyhow::Result<()> {
    let json = crate::engine::shake(args.path)?;
    println!("{json}");
    Ok(())
}

async fn run_index(args: IndexArgs) -> anyhow::Result<()> {
    let json = crate::engine::bake(args.path)?;
    println!("{json}");
    Ok(())
}

async fn run_symbol(args: SymbolArgs) -> anyhow::Result<()> {
    let json = crate::engine::symbol(
        args.path,
        args.name,
        args.include_source,
        args.file,
        args.limit,
        args.stdlib,
    )?;
    println!("{json}");
    Ok(())
}

async fn run_inspect(args: InspectArgs) -> anyhow::Result<()> {
    let json = crate::engine::inspect(
        args.path,
        args.name,
        args.file,
        args.start_line,
        args.end_line,
        Some(args.include_source),
        Some(args.include_summaries),
        args.limit,
        Some(args.stdlib),
        Some(args.signature_only),
        Some(args.type_only),
        args.depth,
    )?;
    println!("{json}");
    Ok(())
}

async fn run_routes(args: RoutesArgs) -> anyhow::Result<()> {
    let json =
        crate::engine::all_endpoints(args.path, args.query, args.method, args.scope, args.limit)?;
    println!("{json}");
    Ok(())
}

async fn run_impact(args: ImpactArgs) -> anyhow::Result<()> {
    let json = crate::engine::impact(
        args.path,
        args.symbol,
        args.endpoint,
        args.method,
        args.depth,
        Some(args.include_source),
        args.scope,
    )?;
    println!("{json}");
    Ok(())
}

async fn run_judge_change(args: JudgeChangeArgs) -> anyhow::Result<()> {
    let json = crate::engine::judge_change(
        args.path,
        args.query,
        args.symbol,
        args.file,
        args.limit,
        args.scope,
    )?;
    println!("{json}");
    Ok(())
}

async fn run_flow(args: FlowArgs) -> anyhow::Result<()> {
    let json = crate::engine::flow(
        args.path,
        args.endpoint,
        args.method,
        args.depth,
        args.include_source,
        args.scope,
    )?;
    println!("{json}");
    Ok(())
}

async fn run_read(args: ReadArgs) -> anyhow::Result<()> {
    let json = crate::engine::slice(args.path, args.file, args.start, args.end)?;
    println!("{json}");
    Ok(())
}

async fn run_outline(args: OutlineArgs) -> anyhow::Result<()> {
    let json = crate::engine::file_functions(
        args.path,
        args.file,
        Some(args.include_summaries),
        args.depth,
    )?;
    println!("{json}");
    Ok(())
}

async fn run_search(args: SearchArgs) -> anyhow::Result<()> {
    let json = crate::engine::supersearch(
        args.path,
        args.query,
        args.context,
        args.pattern,
        Some(args.exclude_tests),
        args.file,
        args.limit,
    )?;
    println!("{json}");
    Ok(())
}

async fn run_module(args: ModuleArgs) -> anyhow::Result<()> {
    let json = crate::engine::package_summary(args.path, args.package)?;
    println!("{json}");
    Ok(())
}

async fn run_map(args: MapArgs) -> anyhow::Result<()> {
    let json = crate::engine::architecture_map(args.path, args.intent, args.limit)?;
    println!("{json}");
    Ok(())
}

async fn run_where(args: WhereArgs) -> anyhow::Result<()> {
    let json = crate::engine::suggest_placement(
        args.path,
        args.function_name,
        args.function_type,
        args.related_to,
    )?;
    println!("{json}");
    Ok(())
}

async fn run_docs(args: DocsArgs) -> anyhow::Result<()> {
    let json = crate::engine::find_docs(args.path, args.doc_type, Some(args.limit))?;
    println!("{json}");
    Ok(())
}

async fn run_change(args: ChangeArgs) -> anyhow::Result<()> {
    let edits = match (args.edits_json, args.edits_file) {
        (Some(_), Some(_)) => anyhow::bail!("Use either --edits-json or --edits-file, not both"),
        (Some(raw), None) => Some(
            serde_json::from_str::<Vec<crate::engine::PatchEdit>>(&raw)
                .map_err(|e| anyhow::anyhow!("Failed to parse --edits-json: {}", e))?,
        ),
        (None, Some(path)) => {
            let raw = std::fs::read_to_string(&path)
                .map_err(|e| anyhow::anyhow!("Failed to read --edits-file '{}': {}", path, e))?;
            Some(
                serde_json::from_str::<Vec<crate::engine::PatchEdit>>(&raw).map_err(|e| {
                    anyhow::anyhow!("Failed to parse --edits-file '{}': {}", path, e)
                })?,
            )
        }
        (None, None) => None,
    };
    let params = match args.params_json {
        Some(raw) => Some(
            serde_json::from_str::<Vec<crate::engine::Param>>(&raw)
                .map_err(|e| anyhow::anyhow!("Failed to parse --params-json: {}", e))?,
        ),
        None => None,
    };
    let json = crate::engine::change(
        args.path,
        args.action,
        args.name,
        args.file,
        args.start_line,
        args.end_line,
        args.new_content,
        args.old_string,
        args.new_string.clone(),
        args.match_index,
        edits,
        args.new_name.or(args.new_string),
        args.to_file,
        Some(args.force),
        args.function_name,
        args.entity_type,
        args.after_symbol,
        args.language,
        params,
        args.returns,
        args.on,
    )?;
    println!("{json}");
    Ok(())
}

async fn run_edit(args: EditArgs) -> anyhow::Result<()> {
    let json = if let Some(name) = args.symbol {
        crate::engine::patch_by_symbol(args.path, name, args.new_content, args.match_index)?
    } else if let (Some(file), Some(start), Some(end)) = (args.file, args.start, args.end) {
        crate::engine::patch(args.path, file, start, end, args.new_content)?
    } else {
        anyhow::bail!(
            "Patch requires either --symbol (patch by symbol name) or --file, --start, and --end (patch by range). See `yoyo patch --help`."
        )
    };
    println!("{json}");
    Ok(())
}

async fn run_retry_plan(args: RetryPlanArgs) -> anyhow::Result<()> {
    let text = match (args.text, args.text_file) {
        (Some(text), None) => text,
        (None, Some(path)) => std::fs::read_to_string(&path).map_err(|err| {
            anyhow::anyhow!("Failed to read retry input file '{}': {}", path, err)
        })?,
        (Some(_), Some(_)) => {
            anyhow::bail!("retry-plan accepts either --text or --text-file, not both")
        }
        (None, None) => anyhow::bail!("retry-plan requires either --text or --text-file"),
    };
    let json = crate::engine::guard_retry_plan(
        args.path,
        text,
        Some(args.max_retries),
        Some(args.context_lines),
    )?;
    println!("{json}");
    Ok(())
}

async fn run_callers(args: CallersArgs) -> anyhow::Result<()> {
    let json = crate::engine::blast_radius(args.path, args.symbol, args.depth)?;
    println!("{json}");
    Ok(())
}

async fn run_rename(args: RenameArgs) -> anyhow::Result<()> {
    let json = crate::engine::graph_rename(args.path, args.name, args.new_name)?;
    println!("{json}");
    Ok(())
}

async fn run_create(args: CreateArgs) -> anyhow::Result<()> {
    let json = crate::engine::graph_create(
        args.path,
        args.file,
        args.function_name,
        args.language,
        None,
        None,
    )?;
    println!("{json}");
    Ok(())
}

async fn run_add(args: AddArgs) -> anyhow::Result<()> {
    let json = crate::engine::graph_add(
        args.path,
        args.entity_type,
        args.name,
        args.file,
        args.after_symbol,
        args.language,
        None,
        None,
        None,
    )?;
    println!("{json}");
    Ok(())
}

async fn run_move(args: MoveArgs) -> anyhow::Result<()> {
    let json = crate::engine::graph_move(args.path, args.name, args.to_file)?;
    println!("{json}");
    Ok(())
}

#[derive(Args, Debug)]
pub struct CallsArgs {
    /// Optional path to the project directory to analyze.
    #[arg(long)]
    pub path: Option<String>,

    /// Function name to start the trace from.
    #[arg(long)]
    pub name: String,

    /// Maximum call depth to follow (default 5).
    #[arg(long)]
    pub depth: Option<usize>,

    /// Optional file path substring to disambiguate when multiple functions share the same name.
    #[arg(long)]
    pub file: Option<String>,
}

async fn run_calls(args: CallsArgs) -> anyhow::Result<()> {
    let json = crate::engine::trace_down(args.path, args.name, args.depth, args.file)?;
    println!("{json}");
    Ok(())
}

#[derive(Args, Debug)]
pub struct HealthArgs {
    /// Optional path to the project directory to analyze.
    #[arg(long)]
    pub path: Option<String>,

    /// Max results per category (default 10).
    #[arg(long)]
    pub top: Option<usize>,

    /// Response view: compact for paged summaries, raw/full for the full payload.
    #[arg(long, value_enum, default_value = "raw")]
    pub view: OutputView,

    /// Items per section when using --view compact (default 3).
    #[arg(long)]
    pub limit: Option<usize>,

    /// Section cursor in the form <section>:<offset>, returned by a previous compact response.
    #[arg(long)]
    pub cursor: Option<String>,
}

async fn run_health(args: HealthArgs) -> anyhow::Result<()> {
    let json = crate::engine::health(
        args.path,
        args.top,
        Some(args.view.as_str().to_string()),
        args.limit,
        args.cursor,
    )?;
    println!("{json}");
    Ok(())
}

#[derive(Args, Debug)]
pub struct DeleteArgs {
    /// Optional path to the project directory to analyze.
    #[arg(long)]
    pub path: Option<String>,

    /// Exact function name to delete.
    #[arg(long)]
    pub name: String,

    /// Optional file path substring to disambiguate when multiple functions share the same name.
    #[arg(long)]
    pub file: Option<String>,

    /// Delete even if active callers exist (default: refuse).
    #[arg(long, default_value_t = false)]
    pub force: bool,
}

async fn run_delete(args: DeleteArgs) -> anyhow::Result<()> {
    let json = crate::engine::graph_delete(args.path, args.name, args.file, args.force)?;
    println!("{json}");
    Ok(())
}

async fn run_ask(args: AskArgs) -> anyhow::Result<()> {
    let json =
        crate::engine::semantic_search(args.path, args.query, args.limit, args.file, args.scope)?;
    println!("{json}");
    Ok(())
}

#[derive(Args, Debug)]
pub struct ScriptArgs {
    /// Optional path to the project directory.
    #[arg(long)]
    pub path: Option<String>,

    /// Rhai script to execute. Inline code string.
    #[arg(long)]
    pub code: Option<String>,

    /// Path to a .rhai file to execute.
    #[arg(long)]
    pub code_file: Option<String>,
}

async fn run_script(args: ScriptArgs) -> anyhow::Result<()> {
    let code = if let Some(c) = args.code {
        c
    } else if let Some(f) = args.code_file {
        std::fs::read_to_string(&f)
            .map_err(|e| anyhow::anyhow!("Failed to read script file '{}': {}", f, e))?
    } else {
        anyhow::bail!("script requires --code <rhai> or --code-file <path>");
    };
    let json = crate::engine::run_script(args.path, code)?;
    println!("{json}");
    Ok(())
}

#[derive(Args, Debug)]
pub struct UpdateArgs {}

async fn run_update(_args: UpdateArgs) -> anyhow::Result<()> {
    eprintln!("Checking for updates...");
    match crate::engine::self_update() {
        Ok(msg) => println!("{msg}"),
        Err(e) => eprintln!("Update failed: {e}"),
    }
    Ok(())
}
