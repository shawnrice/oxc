pub mod apply_renames;
pub mod convert_ast;
pub mod convert_ast_reverse;
pub mod convert_scope;
pub mod diagnostics;
pub mod prefilter;

use std::collections::HashMap;

use apply_renames::build_rename_plan;
use convert_ast::convert_program;
use convert_scope::convert_scope_info;
use diagnostics::compile_result_to_diagnostics;
use prefilter::has_react_like_functions;
use react_compiler::entrypoint::compile_result::LoggerEvent;
use react_compiler::entrypoint::plugin_options::CompilerTarget;
use react_compiler_hir::environment_config::EnvironmentConfig;
// Re-exported so integrations (e.g. `oxc_transformer`) can name the option type
// without depending on the upstream `react_compiler` crate directly.
pub use react_compiler::entrypoint::plugin_options::PluginOptions;

/// A [`PluginOptions`] populated with the compiler's standard defaults.
///
/// `PluginOptions` has no `Default` (the JS plugin pre-resolves several fields),
/// so build options with struct-update syntax, overriding only what you need:
///
/// ```ignore
/// let options = PluginOptions {
///     compilation_mode: "annotation".to_string(),
///     ..default_plugin_options()
/// };
/// ```
///
/// # Options
///
/// The default this function returns is shown in parentheses.
///
/// - **`should_compile`** (`true`) — master on/off switch; the JS plugin resolves
///   it from gating / opt-in. Set `false` to skip the file entirely.
/// - **`compilation_mode`** (`"infer"`) — which functions to compile: `"infer"`
///   (components & hooks, by heuristics), `"syntax"` (by syntactic position),
///   `"annotation"` (only `"use memo"`-annotated functions), or `"all"`.
/// - **`panic_threshold`** (`"none"`) — on a bailout: `"none"` skips the function,
///   `"critical_errors"` throws on critical ones, `"all_errors"` throws on any.
/// - **`target`** (React `"19"`) — runtime target `"17"`, `"18"`, or `"19"` (or a
///   Meta-internal runtime module). 17/18 need the `react-compiler-runtime`
///   package; 19 ships the runtime in `react` itself.
/// - **`no_emit`** (`false`) — analyze and report diagnostics only; emit no code.
/// - **`output_mode`** (`None`) — `"client"`, `"ssr"`, or `"lint"`.
/// - **`ignore_use_no_forget`** (`false`) — when `true`, compile even functions
///   marked `"use no memo"` / `"use no forget"`.
/// - **`custom_opt_out_directives`** (`None`) — extra directives that opt a
///   function out of compilation.
/// - **`gating`** / **`dynamic_gating`** (`None`) — also emit a gated
///   (feature-flagged) version of each compiled function.
/// - **`eslint_suppression_rules`** (`None`) — ESLint rules whose suppressions
///   opt a function out.
/// - **`flow_suppressions`** (`true`) — treat Flow suppression comments as opt-outs.
/// - **`enable_reanimated`** (`false`) — enable `react-native-reanimated` support.
/// - **`is_dev`** (`false`) — development mode (extra validation / instrumentation).
/// - **`filename`** (`None`) — source file name, used for the fast-refresh hash
///   and in diagnostics.
/// - **`environment`** (default) — the large inner `EnvironmentConfig` governing
///   inference, memoization, and validation; see its own docs for the sub-options.
///
/// `source_code`, `profiling`, and `debug` are JS-shim / diagnostic plumbing and
/// stay at their inert defaults.
pub fn default_plugin_options() -> PluginOptions {
    PluginOptions {
        should_compile: true,
        enable_reanimated: false,
        is_dev: false,
        filename: None,
        compilation_mode: "infer".to_string(),
        panic_threshold: "none".to_string(),
        target: CompilerTarget::Version("19".to_string()),
        gating: None,
        dynamic_gating: None,
        no_emit: false,
        output_mode: None,
        eslint_suppression_rules: None,
        flow_suppressions: true,
        ignore_use_no_forget: false,
        custom_opt_out_directives: None,
        environment: EnvironmentConfig::default(),
        source_code: None,
        profiling: false,
        debug: false,
    }
}

/// Result of compiling a program via the OXC frontend.
pub struct TransformResult {
    /// The compiled program as a react_compiler_ast File (None if no changes needed).
    pub file: Option<react_compiler_ast::File>,
    pub diagnostics: Vec<oxc_diagnostics::OxcDiagnostic>,
    pub events: Vec<LoggerEvent>,
    /// Pre-computed rename plan: maps source positions (span.start) to new
    /// identifier names. Built from the compiler's binding renames and the
    /// original scope info. Applied during `emit()` to fix references in
    /// uncompiled sibling functions.
    pub rename_plan: HashMap<u32, String>,
}

/// Result of linting a program via the OXC frontend.
pub struct LintResult {
    pub diagnostics: Vec<oxc_diagnostics::OxcDiagnostic>,
}

/// Primary transform API — accepts pre-parsed OXC AST + semantic.
pub fn transform(
    program: &oxc_ast::ast::Program,
    semantic: &oxc_semantic::Semantic,
    source_text: &str,
    options: PluginOptions,
) -> TransformResult {
    // Prefilter: skip files without React-like functions (unless compilationMode == "all")
    if options.compilation_mode != "all" && !has_react_like_functions(program) {
        return TransformResult {
            file: None,
            diagnostics: vec![],
            events: vec![],
            rename_plan: HashMap::new(),
        };
    }

    // Convert OXC AST to react_compiler_ast
    let file = convert_program(program, source_text);

    // Convert OXC semantic to ScopeInfo
    let scope_info = convert_scope_info(semantic, program);

    // Run the compiler
    let result =
        react_compiler::entrypoint::program::compile_program(file, scope_info.clone(), options);

    let diagnostics = compile_result_to_diagnostics(&result);
    let (program_ast, events, renames) = match result {
        react_compiler::entrypoint::compile_result::CompileResult::Success {
            ast,
            events,
            renames,
            ..
        } => (ast, events, renames),
        react_compiler::entrypoint::compile_result::CompileResult::Error {
            events, ..
        } => (None, events, Vec::new()),
    };

    // Build the rename plan from the original scope info + compiler renames.
    // This maps source positions to new identifier names for uncompiled code.
    let rename_plan = build_rename_plan(&scope_info, &renames);

    let compiled_file = program_ast.and_then(|raw_json| {
        // First parse to serde_json::Value which deduplicates "type" fields
        // (the compiler output can produce duplicate "type" keys due to
        // BaseNode.node_type + #[serde(tag = "type")] enum tagging)
        let value: serde_json::Value = serde_json::from_str(raw_json.get()).ok()?;
        serde_json::from_value(value).ok()
    });

    TransformResult {
        file: compiled_file,
        diagnostics,
        events,
        rename_plan,
    }
}

/// Convenience wrapper — parses source text, runs semantic analysis, then transforms.
pub fn transform_source(
    source_text: &str,
    source_type: oxc_span::SourceType,
    options: PluginOptions,
) -> TransformResult {
    let allocator = oxc_allocator::Allocator::default();
    let parsed = oxc_parser::Parser::new(&allocator, source_text, source_type).parse();

    let semantic = oxc_semantic::SemanticBuilder::new()
        .build(&parsed.program)
        .semantic;

    transform(&parsed.program, &semantic, source_text, options)
}

/// Lint API — accepts pre-parsed OXC AST + semantic.
/// Same as transform but only collects diagnostics, no AST output.
pub fn lint(
    program: &oxc_ast::ast::Program,
    semantic: &oxc_semantic::Semantic,
    source_text: &str,
    options: PluginOptions,
) -> LintResult {
    let mut opts = options;
    opts.no_emit = true;

    let result = transform(program, semantic, source_text, opts);
    LintResult {
        diagnostics: result.diagnostics,
    }
}

/// Emit a react_compiler_ast::File to a string via OXC codegen.
/// Converts the File to an OXC Program, then uses oxc_codegen to emit.
///
/// If `source_text` is provided, comments from the original source will be
/// preserved in the output by re-parsing the source to extract comments and
/// injecting them into the OXC program before codegen.
///
/// If `rename_plan` is non-empty, binding renames are applied to the OXC
/// program before emission. This fixes references in uncompiled sibling
/// functions when the compiler renames a shared binding.
pub fn emit(
    file: &react_compiler_ast::File,
    allocator: &oxc_allocator::Allocator,
    source_text: Option<&str>,
    rename_plan: &HashMap<u32, String>,
) -> String {
    let mut program = if let Some(source) = source_text {
        convert_ast_reverse::convert_program_to_oxc_with_source(file, allocator, source)
    } else {
        convert_ast_reverse::convert_program_to_oxc(file, allocator)
    };

    if let Some(source) = source_text {
        // Re-parse the original source to extract comments.
        // We use a separate allocator for the parse since we only need the comments.
        let comment_allocator = oxc_allocator::Allocator::default();
        // Parse as TSX to handle maximum syntax variety
        let source_type = oxc_span::SourceType::tsx();
        let parsed =
            oxc_parser::Parser::new(&comment_allocator, source, source_type).parse();

        // Collect the span starts of top-level statements in the compiled
        // program. Only comments attached to these positions should be
        // preserved — comments inside function bodies would have
        // `attached_to` values that don't match any top-level statement.
        let mut top_level_starts = std::collections::HashSet::new();
        top_level_starts.insert(0u32); // position 0 for comments at the very start
        for stmt in &program.body {
            use oxc_span::GetSpan;
            let start = stmt.span().start;
            if start > 0 {
                top_level_starts.insert(start);
            }
        }

        // Copy only comments attached to top-level statements.
        let mut comments = oxc_allocator::Vec::with_capacity_in(
            parsed.program.comments.len(),
            allocator,
        );
        for comment in &parsed.program.comments {
            if top_level_starts.contains(&comment.attached_to) {
                comments.push(*comment);
            }
        }
        program.comments = comments;

        // Set the source_text so the codegen can extract comment content
        // from the original source spans.
        // We copy the source into the allocator to guarantee the lifetime.
        let source_in_alloc =
            oxc_allocator::StringBuilder::from_str_in(source, allocator);
        program.source_text = source_in_alloc.into_str();
    }

    // Apply binding renames to fix references in uncompiled sibling functions
    apply_renames::apply_renames(&mut program, rename_plan, allocator);

    oxc_codegen::Codegen::new().build(&program).code
}

/// Convenience wrapper — parses source text, runs semantic analysis, then lints.
pub fn lint_source(
    source_text: &str,
    source_type: oxc_span::SourceType,
    options: PluginOptions,
) -> LintResult {
    let allocator = oxc_allocator::Allocator::default();
    let parsed = oxc_parser::Parser::new(&allocator, source_text, source_type).parse();

    let semantic = oxc_semantic::SemanticBuilder::new()
        .build(&parsed.program)
        .semantic;

    lint(&parsed.program, &semantic, source_text, options)
}

// oxc-added: end-to-end smoke tests (not part of upstream `react_compiler_oxc`).
// Exercise the full pipeline: oxc parse + semantic -> convert -> compile_program
// -> convert back -> oxc codegen, and assert the React Compiler memoization
// artifacts actually appear in the emitted code.
#[cfg(test)]
mod tests {
    use react_compiler::entrypoint::plugin_options::PluginOptions;

    use super::{emit, transform_source};

    fn options() -> PluginOptions {
        // Only the non-`#[serde(default)]` fields are required; the rest
        // (compilationMode "infer", target React 19, environment, ...) default.
        serde_json::from_value(serde_json::json!({
            "shouldCompile": true,
            "enableReanimated": false,
            "isDev": false,
            "filename": "Component.jsx",
        }))
        .unwrap()
    }

    #[test]
    fn memoizes_a_component_end_to_end() {
        let source = "function Component(props) {\n  \
            return <div onClick={() => props.onClick()}>{props.text}</div>;\n}\n";

        let result = transform_source(source, oxc_span::SourceType::tsx(), options());

        assert!(
            result.diagnostics.is_empty(),
            "unexpected diagnostics: {:?}",
            result.diagnostics
        );
        let file = result.file.expect("React Compiler should have transformed the component");

        let allocator = oxc_allocator::Allocator::default();
        let output = emit(&file, &allocator, Some(source), &result.rename_plan);

        // Memoization artifacts proving the full oxc -> RC -> oxc round trip ran.
        assert!(
            output.contains("react/compiler-runtime"),
            "expected the compiler-runtime cache import in output:\n{output}"
        );
        assert!(output.contains("_c("), "expected memo cache reads (`_c(...)`) in output:\n{output}");
    }

    #[test]
    fn skips_non_react_code() {
        // A plain, non-component/non-hook function is filtered out: no change.
        let source = "function add(a, b) {\n  return a + b;\n}\n";
        let result = transform_source(source, oxc_span::SourceType::tsx(), options());
        assert!(result.file.is_none(), "non-React code must not be transformed");
        assert!(result.rename_plan.is_empty());
    }
}
