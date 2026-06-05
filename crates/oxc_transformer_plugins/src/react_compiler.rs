//! React Compiler integration.
//!
//! Runs the Rust port of React Compiler ([facebook/react#36173]), memoizing
//! React components and hooks.
//!
//! This is a standalone pass, not part of the [`Transformer`] traversal: the
//! compiler needs a [`Semantic`](oxc_semantic::Semantic) (it can't work from
//! [`Scoping`] alone — it walks the AST node tree) and rewrites the whole
//! program. It must also see the pristine AST, so the caller is expected to run
//! [`run`] **before** every other transform (JSX, ES lowering, …). [`run`]
//! builds a `Semantic`, runs the compiler, swaps in the result, and rebuilds
//! `Scoping` for the rest of the pipeline.
//!
//! [`Transformer`]: oxc_transformer::Transformer
//! [facebook/react#36173]: https://github.com/facebook/react/pull/36173

use oxc_allocator::Allocator;
use oxc_ast::ast::Program;
use oxc_diagnostics::OxcDiagnostic;
use oxc_semantic::{Scoping, SemanticBuilder};

/// Options for the [React Compiler](https://github.com/facebook/react/pull/36173)
/// (the Rust port) transform — the compiler's concrete, fully-typed `PluginOptions`.
///
/// It has no `Default`; build one with [`default_plugin_options`]
/// (which documents every option, its accepted values, and the defaults) and
/// override fields via struct-update syntax.
pub use oxc_react_compiler::PluginOptions as ReactCompilerOptions;
/// Builds a [`ReactCompilerOptions`] with the React Compiler's standard defaults.
pub use oxc_react_compiler::default_plugin_options;

/// Run the React Compiler over `program`, returning the `Scoping` the rest of the
/// pipeline should use — rebuilt if the program changed, otherwise the input.
pub fn run<'a>(
    program: &mut Program<'a>,
    allocator: &'a Allocator,
    scoping: Scoping,
    options: &ReactCompilerOptions,
    errors: &mut std::vec::Vec<OxcDiagnostic>,
) -> Scoping {
    let source_text = program.source_text;
    let source_type = program.source_type;

    // The compiler needs the AST node tree, so build a `Semantic`. Its borrow of
    // `program` is released at the end of this block, before we replace `*program`.
    let (file, diagnostics, rename_plan) = {
        let semantic = SemanticBuilder::new().build(program).semantic;
        let result =
            oxc_react_compiler::transform(program, &semantic, source_text, options.clone());
        (result.file, result.diagnostics, result.rename_plan)
    };
    errors.extend(diagnostics);

    let Some(file) = file else {
        // No change: `program` is untouched, so the input scoping is still valid.
        return scoping;
    };

    let mut compiled = oxc_react_compiler::convert_ast_reverse::convert_program_to_oxc_with_source(
        &file,
        allocator,
        source_text,
    );
    compiled.source_type = source_type;
    oxc_react_compiler::apply_renames::apply_renames(&mut compiled, &rename_plan, allocator);
    *program = compiled;

    // The compiler rewrote the program; rebuild scoping for the downstream transforms.
    SemanticBuilder::new().build(program).semantic.into_scoping()
}
