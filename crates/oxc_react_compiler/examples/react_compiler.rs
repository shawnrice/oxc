//! # React Compiler Example
//!
//! Runs the Rust port of React Compiler ([facebook/react#36173]) over a file
//! through the oxc frontend (parse + semantic -> convert -> compile -> convert
//! back -> codegen) and prints the memoized output.
//!
//! ## Usage
//!
//! ```bash
//! just example react_compiler             # built-in sample component
//! just example react_compiler MyFile.jsx  # or pass a file path
//! ```
//!
//! [facebook/react#36173]: https://github.com/facebook/react/pull/36173

use std::path::Path;

use oxc_allocator::Allocator;
use oxc_codegen::Codegen;
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::SourceType;
use react_compiler::entrypoint::plugin_options::PluginOptions;

use oxc_react_compiler::transform;

const DEFAULT_SOURCE: &str = "function Component(props) {
  return <div onClick={() => props.onClick()}>{props.text}</div>;
}
";

/// Compile a React component with the Rust React Compiler and print the result.
fn main() {
    let name = std::env::args().nth(1);

    let (source_text, source_type, label) = match &name {
        Some(name) => {
            let path = Path::new(name);
            let source = std::fs::read_to_string(path)
                .unwrap_or_else(|err| panic!("{name} not found.\n{err}"));
            let source_type = SourceType::from_path(path).unwrap_or_else(|_| SourceType::tsx());
            (source, source_type, name.clone())
        }
        None => (DEFAULT_SOURCE.to_string(), SourceType::tsx(), "Component.jsx".to_string()),
    };

    println!("Original ({label}):\n");
    println!("{source_text}");

    // Only the non-`#[serde(default)]` fields are required; the rest
    // (compilationMode "infer", target React 19, environment, ...) default.
    let options: PluginOptions = serde_json::from_value(serde_json::json!({
        "shouldCompile": true,
        "enableReanimated": false,
        "isDev": false,
        "filename": label,
    }))
    .unwrap();

    let allocator = Allocator::default();
    let program = Parser::new(&allocator, &source_text, source_type).parse().program;
    let semantic = SemanticBuilder::new().build(&program).semantic;

    let result = transform(&program, &semantic, &allocator, options);

    if !result.diagnostics.is_empty() {
        println!("Diagnostics:\n");
        for diagnostic in &result.diagnostics {
            println!("{diagnostic:?}");
        }
        println!();
    }

    match result.program {
        Some(compiled) => {
            let output = Codegen::new().build(&compiled).code;
            println!("Compiled:\n");
            println!("{output}");
        }
        None => {
            println!("No changes: no React component or hook found (or compilation bailed out).");
        }
    }
}
