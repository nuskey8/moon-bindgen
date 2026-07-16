//! Generate MoonBit native FFI declarations from Rust source files.
mod generate;
mod model;
mod parse;
mod stub;

use heck::{ToShoutySnakeCase, ToSnakeCase, ToUpperCamelCase};
pub use model::{Bindings, Diagnostic, DiagnosticLevel};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

type OwnershipResolver = dyn Fn(&str, &str) -> Ownership;
type NullabilityResolver = dyn Fn(&str, NullabilityPosition) -> Nullability;

pub struct Builder {
    input_bindgen_files: Vec<PathBuf>,
    input_extern_files: Vec<PathBuf>,
    header: String,
    footer: String,
    c_stub_header: String,
    c_stub_footer: String,
    visibility: Visibility,
    function_filter: fn(String) -> bool,
    type_filter: fn(String) -> bool,
    constant_filter: fn(String) -> bool,
    ownership_resolver: Box<OwnershipResolver>,
    nullability_resolver: Box<NullabilityResolver>,
    function_rename: fn(String) -> String,
    type_rename: fn(String) -> String,
    constant_rename: fn(String) -> String,
}

impl Default for Builder {
    fn default() -> Self {
        Self {
            input_bindgen_files: Vec::new(),
            input_extern_files: Vec::new(),
            header: String::new(),
            footer: String::new(),
            c_stub_header: String::new(),
            c_stub_footer: String::new(),
            visibility: Visibility::Private,
            function_filter: |name| !name.starts_with('_'),
            type_filter: |name| !name.starts_with('_'),
            constant_filter: |name| !name.starts_with('_'),
            ownership_resolver: Box::new(|_, _| Ownership::Borrow),
            nullability_resolver: Box::new(|_, _| Nullability::Unspecified),
            function_rename: |name| name.to_snake_case(),
            type_rename: |name| name.to_upper_camel_case(),
            constant_rename: |name| name.to_shouty_snake_case(),
        }
    }
}

/// Visibility of declarations in the generated MoonBit package.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Visibility {
    #[default]
    Private,
    Public,
}

/// Ownership annotation emitted for a pointer-like MoonBit FFI parameter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Ownership {
    #[default]
    Borrow,
    Owned,
}

/// Whether a C pointer position accepts or produces null.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Nullability {
    /// Use the position-dependent default.
    #[default]
    Unspecified,
    NonNull,
    Nullable,
}

/// A function signature position passed to the nullability resolver.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NullabilityPosition {
    Return,
    Parameter(String),
}

impl Visibility {
    pub(crate) fn prefix(self) -> &'static str {
        match self {
            Self::Private => "",
            Self::Public => "pub ",
        }
    }
}

impl Builder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Input Rust source files that are processed by bindgen.
    pub fn input_bindgen_file<T: AsRef<Path>>(mut self, path: T) -> Self {
        self.input_bindgen_files.push(path.as_ref().to_owned());
        self
    }

    /// Input external Rust source files.
    pub fn input_extern_file<T: AsRef<Path>>(mut self, path: T) -> Self {
        self.input_extern_files.push(path.as_ref().to_owned());
        self
    }

    /// Text emitted before all generated MoonBit declarations.
    pub fn moonbit_file_header(mut self, header: impl Into<String>) -> Self {
        self.header = header.into();
        self
    }

    /// Text emitted after all generated MoonBit declarations.
    pub fn moonbit_file_footer(mut self, footer: impl Into<String>) -> Self {
        self.footer = footer.into();
        self
    }

    /// Text emitted before the generated C stub source. This can be used for
    /// additional `#include` directives, macros, and declarations.
    pub fn c_stub_file_header(mut self, header: impl Into<String>) -> Self {
        self.c_stub_header = header.into();
        self
    }

    /// Text emitted after the generated C stub source.
    pub fn c_stub_file_footer(mut self, footer: impl Into<String>) -> Self {
        self.c_stub_footer = footer.into();
        self
    }

    /// Sets the visibility of all generated types, functions, and constants.
    pub fn moonbit_visibility(mut self, visibility: Visibility) -> Self {
        self.visibility = visibility;
        self
    }

    /// Filters generated functions by their original Rust name.
    pub fn function_filter(mut self, filter: fn(String) -> bool) -> Self {
        self.function_filter = filter;
        self
    }

    /// Filters generated external types and type aliases by their Rust names.
    pub fn type_filter(mut self, filter: fn(String) -> bool) -> Self {
        self.type_filter = filter;
        self
    }

    /// Filters generated constants by their Rust names.
    pub fn constant_filter(mut self, filter: fn(String) -> bool) -> Self {
        self.constant_filter = filter;
        self
    }

    /// Resolves ownership for pointer-like parameters.
    pub fn moonbit_ownership_resolver<F>(mut self, resolver: F) -> Self
    where
        F: Fn(&str, &str) -> Ownership + 'static,
    {
        self.ownership_resolver = Box::new(resolver);
        self
    }

    /// Retained for source compatibility. C pointers now use `nuskey8/c` pointer
    /// types and carry null directly, so this setting no longer changes them.
    #[deprecated(note = "c.mbt pointer types retain null directly")]
    pub fn moonbit_nullability_resolver<F>(mut self, resolver: F) -> Self
    where
        F: Fn(&str, NullabilityPosition) -> Nullability + 'static,
    {
        self.nullability_resolver = Box::new(resolver);
        self
    }

    /// Renames functions. The default converts names to snake_case.
    pub fn moonbit_function_rename(mut self, rename: fn(String) -> String) -> Self {
        self.function_rename = rename;
        self
    }

    /// Renames types. The default converts names to UpperCamelCase.
    pub fn moonbit_type_rename(mut self, rename: fn(String) -> String) -> Self {
        self.type_rename = rename;
        self
    }

    /// Renames constants. The default converts names to SHOUTY_SNAKE_CASE.
    pub fn moonbit_constant_rename(mut self, rename: fn(String) -> String) -> Self {
        self.constant_rename = rename;
        self
    }

    pub fn generate(self) -> Result<Bindings, Error> {
        if self.input_bindgen_files.is_empty() && self.input_extern_files.is_empty() {
            return Err(Error::NoInput);
        }
        let mut model = model::Model::default();
        for path in self
            .input_bindgen_files
            .iter()
            .chain(&self.input_extern_files)
        {
            let source = fs::read_to_string(path).map_err(|source| Error::Read {
                path: path.clone(),
                source,
            })?;
            let syntax = syn::parse_file(&source).map_err(|source| Error::Parse {
                path: path.clone(),
                source,
            })?;
            parse::collect_file(&syntax, &mut model);
        }
        Ok(generate::render(
            &model,
            &self.header,
            &self.footer,
            self.visibility,
            self.function_filter,
            self.type_filter,
            self.constant_filter,
            self.ownership_resolver.as_ref(),
            self.nullability_resolver.as_ref(),
            self.function_rename,
            self.type_rename,
            self.constant_rename,
            false,
            false,
        )
        .with_c_stub_affixes(&self.c_stub_header, &self.c_stub_footer))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("no input files were configured")]
    NoInput,
    #[error("failed to read {path}: {source}")]
    Read { path: PathBuf, source: io::Error },
    #[error("failed to parse Rust source {path}: {source}")]
    Parse { path: PathBuf, source: syn::Error },
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn no_input_is_an_error() {
        assert!(matches!(Builder::new().generate(), Err(Error::NoInput)));
    }

    #[test]
    fn adds_c_stub_file_header_and_footer() {
        let path = std::env::temp_dir().join(format!(
            "moon_bindgen_c_stub_affixes_{}.rs",
            std::process::id()
        ));
        fs::write(
            &path,
            r#"
#[repr(C)] pub struct Value { pub value: i32 }
unsafe extern "C" { pub fn round_trip(value: Value) -> Value; }
"#,
        )
        .unwrap();
        let bindings = Builder::new()
            .input_extern_file(&path)
            .moonbit_file_header("// custom MoonBit header")
            .c_stub_file_header("#include \"custom.h\"")
            .c_stub_file_footer("// custom footer")
            .generate()
            .unwrap();
        let _ = fs::remove_file(path);
        assert!(
            bindings
                .c_stub_source()
                .starts_with("// Generated by moon-bindgen. Do not edit.\n#include \"custom.h\"")
        );
        assert!(bindings.c_stub_source().ends_with("// custom footer\n"));
        assert!(
            bindings.moonbit_source().starts_with(
                "// Generated by moon-bindgen. Do not edit.\n// custom MoonBit header"
            )
        );
    }

    #[test]
    fn pointers_use_the_shared_c_package() {
        let path =
            std::env::temp_dir().join(format!("moon_bindgen_c_pointer_{}.rs", std::process::id()));
        fs::write(
            &path,
            r#"unsafe extern "C" {
  pub fn bytes(input: *const u8, output: *mut u8);
  pub fn byte_pointer() -> *const u8;
  pub fn opaque(pointer: *mut core::ffi::c_void) -> *const core::ffi::c_void;
}"#,
        )
        .unwrap();
        let pointers = Builder::new().input_extern_file(&path).generate().unwrap();
        let _ = fs::remove_file(path);
        assert!(
            pointers
                .moonbit_source()
                .contains("input : @c.ReadOnlyPointer[@c.CUInt8]")
        );
        assert!(
            pointers
                .moonbit_source()
                .contains("output : @c.Pointer[@c.CUInt8]")
        );
        assert!(
            pointers
                .moonbit_source()
                .contains("pointer : @c.Pointer[@c.CVoid]")
        );
        assert!(
            pointers
                .moonbit_source()
                .contains("-> @c.ReadOnlyPointer[@c.CVoid]")
        );
        assert!(!pointers.moonbit_source().contains("type Ptr[T]"));
        assert!(!pointers.moonbit_source().contains("Bytes"));
        assert!(!pointers.c_stub_source().contains("moon_bindgen_ptr_"));
    }
}
