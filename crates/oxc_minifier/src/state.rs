use oxc_ecmascript::constant_evaluation::ConstantValue;
use rustc_hash::{FxHashMap, FxHashSet};

use oxc_data_structures::stack::NonEmptyStack;
use oxc_semantic::Scoping;
use oxc_span::SourceType;
use oxc_str::Str;
use oxc_syntax::{scope::ScopeId, symbol::SymbolId};

use crate::{CompressOptions, symbol_value::SymbolValues};

pub struct MinifierState<'a> {
    pub source_type: SourceType,

    pub options: CompressOptions,

    /// When true, only run dead code elimination passes (subset of full peephole optimizations).
    pub dce: bool,

    /// The return value of function declarations that are pure
    pub pure_functions: FxHashMap<SymbolId, Option<ConstantValue<'a>>>,

    pub symbol_values: SymbolValues<'a>,

    /// Private member usage for classes
    pub class_symbols_stack: ClassSymbolsStack<'a>,

    /// Symbols that have `__proto__` member writes.
    /// Writing to `__proto__` changes the prototype chain, potentially installing
    /// setters that make subsequent property writes side-effectful.
    pub proto_write_symbols: FxHashSet<SymbolId>,

    /// One frame per enclosing function body (program root at the bottom).
    /// `(body_scope, saw_non_declarative_stmt)`. While `.1` is false, the next
    /// `var x = <literal>;` whose declarator sits at `.0` is safe to inline
    /// despite hoisting. Pushed by `enter_function_body`, popped by
    /// `exit_function_body`. See `init_symbol_value`.
    pub body_unsafe_stack: NonEmptyStack<(ScopeId, bool)>,

    /// True when the program body contains any module-loader statement: a
    /// static `import`, `export * from`, or `export … from`. All three are
    /// hoisted and trigger evaluation of a foreign module wherever they appear
    /// in source. Set once in `enter_program`. When `true`, the program-scope
    /// var-inlining path bails: a cyclic importer can observe any binding our
    /// exported functions/classes close over, regardless of whether the var
    /// itself is exported.
    pub module_has_loaders: bool,

    pub changed: bool,
}

impl MinifierState<'_> {
    pub fn new(
        source_type: SourceType,
        options: CompressOptions,
        dce: bool,
        scoping: &Scoping,
    ) -> Self {
        Self {
            source_type,
            options,
            dce,
            pure_functions: FxHashMap::default(),
            symbol_values: SymbolValues::new(scoping.symbols_len()),
            class_symbols_stack: ClassSymbolsStack::new(),
            proto_write_symbols: FxHashSet::default(),
            body_unsafe_stack: NonEmptyStack::new((scoping.root_scope_id(), false)),
            module_has_loaders: false,
            changed: false,
        }
    }
}

/// Stack to track class symbol information
pub struct ClassSymbolsStack<'a> {
    stack: NonEmptyStack<FxHashSet<Str<'a>>>,
}

impl<'a> ClassSymbolsStack<'a> {
    pub fn new() -> Self {
        Self { stack: NonEmptyStack::new(FxHashSet::default()) }
    }

    /// Check if the stack is exhausted
    pub fn is_exhausted(&self) -> bool {
        self.stack.is_exhausted()
    }

    /// Enter a new class scope
    pub fn push_class_scope(&mut self) {
        self.stack.push(FxHashSet::default());
    }

    /// Exit the current class scope
    pub fn pop_class_scope(&mut self, declared_private_symbols: impl Iterator<Item = Str<'a>>) {
        let mut used_private_symbols = self.stack.pop();
        declared_private_symbols.for_each(|name| {
            used_private_symbols.remove(&name);
        });
        // if the symbol was not declared in this class, that is declared in the class outside the current class
        self.stack.last_mut().extend(used_private_symbols);
    }

    /// Add a private member to the current class scope
    pub fn push_private_member_to_current_class(&mut self, name: Str<'a>) {
        self.stack.last_mut().insert(name);
    }

    /// Check if a private member is used in the current class scope
    pub fn is_private_member_used_in_current_class(&self, name: &Str<'a>) -> bool {
        self.stack.last().contains(name)
    }
}
