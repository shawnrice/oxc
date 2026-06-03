use oxc_span::SourceType;
use oxc_transformer::TransformOptions;

use crate::test_with_source_type;

#[test]
fn preserves_source_type_for_downstream_transforms() {
    let source =
        "function Component(props: { value: number }) { return <div>{props.value}</div>; }";
    let options = TransformOptions {
        react_compiler: Some(oxc_transformer::default_plugin_options()),
        ..TransformOptions::default()
    };

    let code = test_with_source_type(source, SourceType::tsx(), &options)
        .expect("transform should succeed");

    assert!(!code.contains(": { value: number }"));
    assert!(!code.contains("<div>"));
}
