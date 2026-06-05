use oxc_formatter_json::{
    BracketSpacing, Expand, JsonFormatOptions, JsonVariant, QuoteProps, TrailingCommas,
};

use super::{
    super::oxfmtrc::{FormatConfig, ObjectWrapConfig, QuotePropsConfig, TrailingCommaConfig},
    to_core_options::to_core_options,
};

/// Convert `FormatConfig` into validated `JsonFormatOptions` for `oxc_formatter_json`.
///
/// Most JSON-specific output options are fixed by [`oxc_formatter_json::JsonVariant`].
/// The exceptions:
/// - `trailingComma`: honored by `jsonc`/`json5` (the `json` variant ignores it, matching
///   Prettier forcing `"none"`).
/// - `singleQuote` / `quoteProps`: honored by `json5` only (other variants always emit
///   double-quoted strings and quoted keys).
///
/// # Errors
/// Returns error if any option value is invalid.
pub fn to_oxc_formatter_json(
    config: &FormatConfig,
    variant: JsonVariant,
) -> Result<JsonFormatOptions, String> {
    let core = to_core_options(config)?;

    let mut options = JsonFormatOptions {
        variant,
        indent_style: core.indent_style,
        indent_width: core.indent_width,
        line_width: core.line_width,
        line_ending: core.line_ending,
        ..JsonFormatOptions::default()
    };

    // [Prettier] trailingComma: "all" | "es5" | "none" (default "all").
    // Only honored by `jsonc`/`json5`, `json`/`json-stringify` ignore it.
    // And "all" and "es5" are indistinguishable for JSON.
    if let Some(commas) = config.trailing_comma {
        options.trailing_commas = match commas {
            TrailingCommaConfig::All | TrailingCommaConfig::Es5 => TrailingCommas::Always,
            TrailingCommaConfig::None => TrailingCommas::Never,
        };
    }
    // [Prettier] bracketSpacing: boolean
    if let Some(spacing) = config.bracket_spacing {
        options.bracket_spacing = BracketSpacing::from(spacing);
    }
    // [Prettier] objectWrap: "preserve" | "collapse"
    if let Some(wrap) = config.object_wrap {
        options.expand = match wrap {
            ObjectWrapConfig::Preserve => Expand::Auto,
            ObjectWrapConfig::Collapse => Expand::Never,
        };
    }
    // [Prettier] singleQuote: boolean (default false). Only honored by `json5`.
    if let Some(single_quote) = config.single_quote {
        options.single_quote = single_quote;
    }
    // [Prettier] quoteProps: "as-needed" | "consistent" | "preserve" (default "as-needed").
    // Only honored by `json5`.
    if let Some(quote_props) = config.quote_props {
        options.quote_props = match quote_props {
            QuotePropsConfig::AsNeeded => QuoteProps::AsNeeded,
            QuotePropsConfig::Consistent => QuoteProps::Consistent,
            QuotePropsConfig::Preserve => QuoteProps::Preserve,
        };
    }

    Ok(options)
}
