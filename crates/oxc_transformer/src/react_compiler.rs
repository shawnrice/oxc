//! React Compiler integration.
//!
//! Runs the Rust port of React Compiler ([facebook/react#36173]) as the **first**
//! transform, before JSX and ES lowering, memoizing React components and hooks.
//!
//! The compiler needs a [`Semantic`](oxc_semantic::Semantic) (it can't work from
//! [`Scoping`] alone — it walks the AST node tree) and rewrites the whole program,
//! so this builds a `Semantic`, runs the compiler, swaps in the result, and
//! rebuilds `Scoping` for the rest of the pipeline. It must see the pristine AST,
//! which is why it runs ahead of every other transform.
//!
//! [facebook/react#36173]: https://github.com/facebook/react/pull/36173

use oxc_allocator::Allocator;
use oxc_ast::ast::Program;
use oxc_diagnostics::OxcDiagnostic;
use oxc_semantic::{Scoping, SemanticBuilder};

use crate::options::ReactCompilerOptions;

/// Run the React Compiler over `program`, returning the `Scoping` the rest of the
/// pipeline should use — rebuilt if the program changed, otherwise the input.
pub fn run<'a>(
    program: &mut Program<'a>,
    allocator: &'a Allocator,
    scoping: Scoping,
    options: &ReactCompilerOptions,
    errors: &mut std::vec::Vec<OxcDiagnostic>,
) -> Scoping {
    let plugin_options = match resolve_plugin_options(&options.plugin_options) {
        Ok(plugin_options) => plugin_options,
        Err(err) => {
            errors.push(OxcDiagnostic::error(format!(
                "react_compiler: invalid plugin options: {err}"
            )));
            return scoping;
        }
    };

    let source_text = program.source_text;

    // The compiler needs the AST node tree, so build a `Semantic`. Its borrow of
    // `program` is released at the end of this block, before we replace `*program`.
    let (file, diagnostics, rename_plan) = {
        let semantic = SemanticBuilder::new().build(program).semantic;
        let result = oxc_react_compiler::transform(program, &semantic, source_text, plugin_options);
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
    oxc_react_compiler::apply_renames::apply_renames(&mut compiled, &rename_plan, allocator);
    *program = compiled;

    // The compiler rewrote the program; rebuild scoping for the downstream transforms.
    SemanticBuilder::new().build(program).semantic.into_scoping()
}

/// Build [`oxc_react_compiler::PluginOptions`] from the passed-through JSON,
/// filling the fields the JS plugin normally pre-resolves so a default (empty)
/// value still compiles every component.
fn resolve_plugin_options(
    value: &serde_json::Value,
) -> Result<oxc_react_compiler::PluginOptions, serde_json::Error> {
    let mut merged = serde_json::json!({
        "shouldCompile": true,
        "enableReanimated": false,
        "isDev": false,
        "filename": null,
    });
    if let (serde_json::Value::Object(base), serde_json::Value::Object(user)) = (&mut merged, value)
    {
        for (key, value) in user {
            base.insert(key.clone(), value.clone());
        }
    }
    serde_json::from_value(merged)
}
