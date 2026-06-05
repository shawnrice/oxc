use std::path::Path;

use oxc_allocator::Allocator;
use oxc_codegen::Codegen;
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::SourceType;
use oxc_transformer::{TransformOptions, Transformer};
use oxc_transformer_plugins::react_compiler::{self, default_plugin_options};

#[test]
fn preserves_source_type_for_downstream_transforms() {
    let source =
        "function Component(props: { value: number }) { return <div>{props.value}</div>; }";
    let source_type = SourceType::tsx();

    let allocator = Allocator::default();
    let mut program = Parser::new(&allocator, source, source_type).parse().program;
    let scoping = SemanticBuilder::new().build(&program).semantic.into_scoping();

    // React Compiler runs first, on the pristine AST, before every other transform.
    let mut errors = Vec::new();
    let scoping = react_compiler::run(
        &mut program,
        &allocator,
        scoping,
        &default_plugin_options(),
        &mut errors,
    );
    assert!(errors.is_empty(), "unexpected react compiler diagnostics: {errors:?}");

    let options = TransformOptions::default();
    let ret = Transformer::new(&allocator, Path::new(""), &options)
        .build_with_scoping(scoping, &mut program);
    assert!(ret.errors.is_empty(), "unexpected transform diagnostics: {:?}", ret.errors);

    let code = Codegen::new().build(&program).code;

    // The React Compiler swaps in a freshly built program; it must carry the
    // original source type so the downstream TS + JSX passes still lower both.
    assert!(!code.contains(": { value: number }"), "TS types should be stripped:\n{code}");
    assert!(!code.contains("<div>"), "JSX should be lowered:\n{code}");
}
