use clap::{Args, Subcommand};

/// High-level yoyo commands exposed to humans.
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Prime directive and usage instructions for yoyo.
    LlmInstructions(LlmInstructionsArgs),
    /// Repository overview similar to Shake.readme.
    Shake(ShakeArgs),
    /// Build and persist a bake index under the project root.
    Bake(BakeArgs),
    /// Detailed lookup of a function symbol from the bake index.
    Symbol(SymbolArgs),
    /// List all detected API endpoints from the bake index.
    AllEndpoints(AllEndpointsArgs),
    /// Read a specific line range of a file.
    Slice(SliceArgs),
    /// Exported API summary grouped by module (TypeScript-only for now).
    ApiSurface(ApiSurfaceArgs),
    /// Per-file function overview from the bake index.
    FileFunctions(FileFunctionsArgs),
    /// Text-based search over TS/JS source files.
    Supersearch(SupersearchArgs),
    /// Deep-dive summary of a package/module directory.
    PackageSummary(PackageSummaryArgs),
    /// Project structure map and placement hints.
    ArchitectureMap(ArchitectureMapArgs),
    /// Suggest where to place a new function.
    SuggestPlacement(SuggestPlacementArgs),
    /// Entity-level CRUD matrix inferred from endpoints.
    CrudOperations(CrudOperationsArgs),
    /// Trace an API endpoint through backend handlers.
    ApiTrace(ApiTraceArgs),
    /// Find documentation/config files.
    FindDocs(FindDocsArgs),
    /// Apply a patch by symbol name or by file/line range.
    Patch(PatchArgs),
    /// Analyse the blast radius of a symbol (transitive callers + affected files).
    BlastRadius(BlastRadiusArgs),
    /// Rename a symbol everywhere (definition + all call sites) atomically.
    GraphRename(GraphRenameArgs),
    /// Insert a new function scaffold into a file.
    GraphAdd(GraphAddArgs),
    /// Move a function from one file to another.
    /// Move a function from one file to another.
    GraphMove(GraphMoveArgs),
    /// Trace a function's call chain downward to external boundaries.
    TraceDown(TraceDownArgs),
    /// Audit dead code, god functions, and duplicate hints.
    Health(HealthArgs),
    /// Remove a function from a file by name.
    GraphDelete(GraphDeleteArgs),
}

#[derive(Args, Debug)]
pub struct LlmInstructionsArgs {
    /// Optional path to the project directory to analyze.
    #[arg(long)]
    pub path: Option<String>,
}

#[derive(Args, Debug)]
pub struct ShakeArgs {
    /// Optional path to the project directory to analyze.
    #[arg(long)]
    pub path: Option<String>,
}

#[derive(Args, Debug)]
pub struct BakeArgs {
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
}

#[derive(Args, Debug)]
pub struct AllEndpointsArgs {
    /// Optional path to the project directory to analyze.
    #[arg(long)]
    pub path: Option<String>,
}

#[derive(Args, Debug)]
pub struct SliceArgs {
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
pub struct ApiSurfaceArgs {
    /// Optional path to the project directory to analyze.
    #[arg(long)]
    pub path: Option<String>,

    /// Optional package/module filter (substring match on module or file paths).
    #[arg(long)]
    pub package: Option<String>,

    /// Maximum number of functions per module (default 20).
    #[arg(long, default_value_t = 20)]
    pub limit: usize,
}

#[derive(Args, Debug)]
pub struct FileFunctionsArgs {
    /// Optional path to the project directory to analyze.
    #[arg(long)]
    pub path: Option<String>,

    /// File path relative to the project root.
    #[arg(long)]
    pub file: String,

    /// Whether to include summaries (currently a no-op placeholder).
    #[arg(long, default_value_t = true)]
    pub include_summaries: bool,
}

#[derive(Args, Debug)]
pub struct SupersearchArgs {
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
pub struct PackageSummaryArgs {
    /// Optional path to the project directory to analyze.
    #[arg(long)]
    pub path: Option<String>,

    /// Package/module name or directory substring.
    #[arg(long)]
    pub package: String,
}

#[derive(Args, Debug)]
pub struct ArchitectureMapArgs {
    /// Optional path to the project directory to analyze.
    #[arg(long)]
    pub path: Option<String>,

    /// Intent description, e.g. "user handler" or "auth service".
    #[arg(long)]
    pub intent: Option<String>,
}

#[derive(Args, Debug)]
pub struct SuggestPlacementArgs {
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
pub struct CrudOperationsArgs {
    /// Optional path to the project directory to analyze.
    #[arg(long)]
    pub path: Option<String>,

    /// Optional entity filter (e.g. "user").
    #[arg(long)]
    pub entity: Option<String>,
}

#[derive(Args, Debug)]
pub struct ApiTraceArgs {
    /// Optional path to the project directory to analyze.
    #[arg(long)]
    pub path: Option<String>,

    /// Endpoint path (or substring), e.g. "/users".
    #[arg(long)]
    pub endpoint: String,

    /// Optional HTTP method (GET, POST, etc.).
    #[arg(long)]
    pub method: Option<String>,
}

#[derive(Args, Debug)]
pub struct FindDocsArgs {
    /// Optional path to the project directory to analyze.
    #[arg(long)]
    pub path: Option<String>,

    /// Documentation type: readme | env | config | docker | all.
    #[arg(long)]
    pub doc_type: String,

    /// Maximum number of results to return (default 50).
    #[arg(long, default_value_t = 50)]
    pub limit: usize,
}

#[derive(Args, Debug)]
pub struct PatchArgs {
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
pub struct BlastRadiusArgs {
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
pub struct GraphRenameArgs {
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
pub struct GraphAddArgs {
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
pub struct GraphMoveArgs {
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

pub async fn run(command: Option<Command>) -> anyhow::Result<()> {
    match command {
        Some(Command::LlmInstructions(args)) => run_llm_instructions(args).await?,
        Some(Command::Shake(args)) => run_shake(args).await?,
        Some(Command::Bake(args)) => run_bake(args).await?,
        Some(Command::Symbol(args)) => run_symbol(args).await?,
        Some(Command::AllEndpoints(args)) => run_all_endpoints(args).await?,
        Some(Command::Slice(args)) => run_slice(args).await?,
        Some(Command::ApiSurface(args)) => run_api_surface(args).await?,
        Some(Command::FileFunctions(args)) => run_file_functions(args).await?,
        Some(Command::Supersearch(args)) => run_supersearch(args).await?,
        Some(Command::PackageSummary(args)) => run_package_summary(args).await?,
        Some(Command::ArchitectureMap(args)) => run_architecture_map(args).await?,
        Some(Command::SuggestPlacement(args)) => run_suggest_placement(args).await?,
        Some(Command::CrudOperations(args)) => run_crud_operations(args).await?,
        Some(Command::ApiTrace(args)) => run_api_trace(args).await?,
        Some(Command::FindDocs(args)) => run_find_docs(args).await?,
        Some(Command::Patch(args)) => run_patch(args).await?,
        Some(Command::BlastRadius(args)) => run_blast_radius(args).await?,
        Some(Command::GraphRename(args)) => run_graph_rename(args).await?,
        Some(Command::GraphAdd(args)) => run_graph_add(args).await?,
        Some(Command::GraphMove(args)) => run_graph_move(args).await?,
        Some(Command::TraceDown(args)) => run_trace_down(args).await?,
        Some(Command::Health(args)) => run_health(args).await?,
        Some(Command::GraphDelete(args)) => run_graph_delete(args).await?,
        None => {
            eprintln!(
                "No command provided. Run `yoyo --help` for available commands."
            );
        }
    }
    Ok(())
}

async fn run_llm_instructions(args: LlmInstructionsArgs) -> anyhow::Result<()> {
    let json = crate::engine::llm_instructions(args.path)?;
    println!("{json}");
    Ok(())
}

async fn run_shake(args: ShakeArgs) -> anyhow::Result<()> {
    let json = crate::engine::shake(args.path)?;
    println!("{json}");
    Ok(())
}

async fn run_bake(args: BakeArgs) -> anyhow::Result<()> {
    let json = crate::engine::bake(args.path)?;
    println!("{json}");
    Ok(())
}

async fn run_symbol(args: SymbolArgs) -> anyhow::Result<()> {
    let json = crate::engine::symbol(args.path, args.name, args.include_source, args.file, args.limit)?;
    println!("{json}");
    Ok(())
}

async fn run_all_endpoints(args: AllEndpointsArgs) -> anyhow::Result<()> {
    let json = crate::engine::all_endpoints(args.path)?;
    println!("{json}");
    Ok(())
}

async fn run_slice(args: SliceArgs) -> anyhow::Result<()> {
    let json = crate::engine::slice(args.path, args.file, args.start, args.end)?;
    println!("{json}");
    Ok(())
}

async fn run_api_surface(args: ApiSurfaceArgs) -> anyhow::Result<()> {
    let json = crate::engine::api_surface(args.path, args.package, Some(args.limit))?;
    println!("{json}");
    Ok(())
}

async fn run_file_functions(args: FileFunctionsArgs) -> anyhow::Result<()> {
    let json =
        crate::engine::file_functions(args.path, args.file, Some(args.include_summaries))?;
    println!("{json}");
    Ok(())
}

async fn run_supersearch(args: SupersearchArgs) -> anyhow::Result<()> {
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

async fn run_package_summary(args: PackageSummaryArgs) -> anyhow::Result<()> {
    let json = crate::engine::package_summary(args.path, args.package)?;
    println!("{json}");
    Ok(())
}

async fn run_architecture_map(args: ArchitectureMapArgs) -> anyhow::Result<()> {
    let json = crate::engine::architecture_map(args.path, args.intent)?;
    println!("{json}");
    Ok(())
}

async fn run_suggest_placement(args: SuggestPlacementArgs) -> anyhow::Result<()> {
    let json = crate::engine::suggest_placement(
        args.path,
        args.function_name,
        args.function_type,
        args.related_to,
    )?;
    println!("{json}");
    Ok(())
}

async fn run_crud_operations(args: CrudOperationsArgs) -> anyhow::Result<()> {
    let json = crate::engine::crud_operations(args.path, args.entity)?;
    println!("{json}");
    Ok(())
}

async fn run_api_trace(args: ApiTraceArgs) -> anyhow::Result<()> {
    let json = crate::engine::api_trace(args.path, args.endpoint, args.method)?;
    println!("{json}");
    Ok(())
}

async fn run_find_docs(args: FindDocsArgs) -> anyhow::Result<()> {
    let json = crate::engine::find_docs(args.path, args.doc_type, Some(args.limit))?;
    println!("{json}");
    Ok(())
}

async fn run_patch(args: PatchArgs) -> anyhow::Result<()> {
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

async fn run_blast_radius(args: BlastRadiusArgs) -> anyhow::Result<()> {
    let json = crate::engine::blast_radius(args.path, args.symbol, args.depth)?;
    println!("{json}");
    Ok(())
}

async fn run_graph_rename(args: GraphRenameArgs) -> anyhow::Result<()> {
    let json = crate::engine::graph_rename(args.path, args.name, args.new_name)?;
    println!("{json}");
    Ok(())
}

async fn run_graph_add(args: GraphAddArgs) -> anyhow::Result<()> {
    let json = crate::engine::graph_add(
        args.path,
        args.entity_type,
        args.name,
        args.file,
        args.after_symbol,
        args.language,
    )?;
    println!("{json}");
    Ok(())
}

async fn run_graph_move(args: GraphMoveArgs) -> anyhow::Result<()> {
    let json = crate::engine::graph_move(args.path, args.name, args.to_file)?;
    println!("{json}");
    Ok(())
}

#[derive(Args, Debug)]
pub struct TraceDownArgs {
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

async fn run_trace_down(args: TraceDownArgs) -> anyhow::Result<()> {
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
}

async fn run_health(args: HealthArgs) -> anyhow::Result<()> {
    let json = crate::engine::health(args.path, args.top)?;
    println!("{json}");
    Ok(())
}

#[derive(Args, Debug)]
pub struct GraphDeleteArgs {
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

async fn run_graph_delete(args: GraphDeleteArgs) -> anyhow::Result<()> {
    let json = crate::engine::graph_delete(args.path, args.name, args.file, args.force)?;
    println!("{json}");
    Ok(())
}
