use std::{collections::BTreeMap, fs, io, path::Path};

#[derive(Default)]
pub(crate) struct Model {
    pub functions: BTreeMap<String, Function>,
    pub structs: BTreeMap<String, Struct>,
    pub aliases: BTreeMap<String, Type>,
    pub constants: BTreeMap<String, Constant>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone)]
pub(crate) struct Function {
    pub rust_name: String,
    pub symbol: String,
    pub params: Vec<(String, Type)>,
    pub result: Type,
    pub variadic: bool,
}

#[derive(Clone)]
pub(crate) struct Struct {
    pub name: String,
    pub is_union: bool,
    pub fields: Vec<Field>,
}

#[derive(Clone)]
pub(crate) struct Field {
    pub name: String,
    pub ty: Type,
}

#[derive(Clone, Debug)]
pub(crate) enum Type {
    Unit,
    Path(String),
    Pointer {
        inner: Box<Type>,
        mutable: bool,
    },
    Array {
        inner: Box<Type>,
        len: Option<usize>,
    },
    FunctionPointer {
        params: Vec<Type>,
        result: Box<Type>,
        variadic: bool,
        nullable: bool,
    },
    Unsupported,
}

pub(crate) struct Constant {
    pub ty: Type,
    pub value: String,
}

pub struct Bindings {
    source: String,
    c_stub: String,
    diagnostics: Vec<Diagnostic>,
}

impl Bindings {
    pub(crate) fn new(source: String, c_stub: String, diagnostics: Vec<Diagnostic>) -> Self {
        Self {
            source,
            c_stub,
            diagnostics,
        }
    }

    pub(crate) fn with_c_stub_affixes(mut self, header: &str, footer: &str) -> Self {
        if header.is_empty() && footer.is_empty() {
            return self;
        }
        let mut source = String::new();
        if !header.is_empty() {
            source.push_str(header);
            if !header.ends_with('\n') {
                source.push('\n');
            }
        }
        source.push_str(&self.c_stub);
        if !footer.is_empty() {
            if !source.is_empty() && !source.ends_with('\n') {
                source.push('\n');
            }
            source.push_str(footer);
            if !footer.ends_with('\n') {
                source.push('\n');
            }
        }
        self.c_stub = source;
        self
    }

    /// Returns the generated MoonBit source.
    pub fn moonbit_source(&self) -> &str {
        &self.source
    }

    /// Returns diagnostics for declarations that were skipped or need review.
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// Returns the generated C ABI adapter source.
    pub fn c_stub_source(&self) -> &str {
        &self.c_stub
    }

    /// Writes the generated MoonBit source and C ABI adapter source to files.
    pub fn write_to_file(
        &self,
        moonbit_ffi_path: impl AsRef<Path>,
        c_stub_path: impl AsRef<Path>,
    ) -> io::Result<()> {
        fs::write(moonbit_ffi_path, &self.source).and_then(|_| fs::write(c_stub_path, &self.c_stub))
    }

    /// Writes the generated MoonBit source.
    pub fn write_moonbit_to_file(&self, path: impl AsRef<Path>) -> io::Result<()> {
        fs::write(path, &self.source)
    }

    /// Writes the generated C ABI adapter source.
    pub fn write_c_stub_to_file(&self, path: impl AsRef<Path>) -> io::Result<()> {
        fs::write(path, &self.c_stub)
    }
}

impl std::fmt::Display for Bindings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.source)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub level: DiagnosticLevel,
    pub item: String,
    pub message: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiagnosticLevel {
    Warning,
}
