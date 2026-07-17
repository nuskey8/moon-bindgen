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
    function_rename: fn(String) -> String,
    type_rename: fn(String) -> String,
    constant_rename: fn(String) -> String,
    prefer_managed_types: bool,
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
            function_rename: |name| name.to_snake_case(),
            type_rename: |name| name.to_upper_camel_case(),
            constant_rename: |name| name.to_shouty_snake_case(),
            prefer_managed_types: false,
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

    /// Prefers MoonBit-managed FFI parameter types where their C ABI and
    /// lifetime semantics are compatible. Currently, read-only byte and char
    /// pointers become `Bytes`, while writable byte and char pointers become
    /// `FixedArray[Byte]`.
    pub fn moonbit_prefer_managed_types(mut self, prefer: bool) -> Self {
        self.prefer_managed_types = prefer;
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
        for (path, from_bindgen) in self
            .input_bindgen_files
            .iter()
            .map(|path| (path, true))
            .chain(self.input_extern_files.iter().map(|path| (path, false)))
        {
            let source = fs::read_to_string(path).map_err(|source| Error::Read {
                path: path.clone(),
                source,
            })?;
            let syntax = syn::parse_file(&source).map_err(|source| Error::Parse {
                path: path.clone(),
                source,
            })?;
            if from_bindgen {
                parse::collect_bindgen_file(&syntax, &mut model);
            } else {
                parse::collect_file(&syntax, &mut model);
            }
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
            self.function_rename,
            self.type_rename,
            self.constant_rename,
            self.prefer_managed_types,
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

    #[test]
    fn can_prefer_managed_byte_parameter_types_without_a_stub() {
        let path = std::env::temp_dir().join(format!(
            "moon_bindgen_managed_byte_parameters_{}.rs",
            std::process::id()
        ));
        fs::write(
            &path,
            r#"
pub type ByteAlias = u8;
unsafe extern "C" {
  pub fn process(
    input: *const u8,
    output: *mut ByteAlias,
    text: *const core::ffi::c_char,
    words: *const u32,
  );
  pub fn borrowed_result() -> *const u8;
}
"#,
        )
        .unwrap();
        let bindings = Builder::new()
            .input_extern_file(&path)
            .moonbit_prefer_managed_types(true)
            .generate()
            .unwrap();
        let _ = fs::remove_file(path);
        let moon = bindings.moonbit_source();
        assert!(moon.contains("input : Bytes"));
        assert!(moon.contains("output : FixedArray[Byte]"));
        assert!(moon.contains("text : Bytes"));
        assert!(moon.contains("words : @c.ReadOnlyPointer[@c.CUInt32]"));
        assert!(moon.contains("-> @c.ReadOnlyPointer[@c.CUInt8]"));
        assert!(bindings.c_stub_source().is_empty());
    }

    #[test]
    fn native_layout_stub_compiles_against_the_supplied_c_header() {
        let dir =
            std::env::temp_dir().join(format!("moon_bindgen_native_layout_{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("moonbit.h"),
            r#"
#include <stddef.h>
#define MOONBIT_FFI_EXPORT
void *moonbit_make_external_object(void *finalizer, size_t size);
"#,
        )
        .unwrap();
        fs::write(
            dir.join("library.h"),
            r#"
typedef struct {
  long platform_word;
  void *platform_data;
} PlatformInfo;
typedef struct SockAddr SockAddr;
typedef struct {
  SockAddr *from;
  unsigned int from_len;
  SockAddr *to;
  unsigned int to_len;
} RecvInfo;
PlatformInfo round_trip_platform_info(PlatformInfo value);
int receive_packet(const RecvInfo *info);
"#,
        )
        .unwrap();
        fs::write(
            dir.join("bindings.rs"),
            r#"
#[repr(C)]
pub struct PlatformInfo {
  pub _bindgen_opaque_blob: [u8; 16],
}
#[repr(C)]
pub struct SockAddr {
  pub _bindgen_opaque_blob: [u8; 128],
}
#[repr(C)]
pub struct RecvInfo {
  pub from: *mut SockAddr,
  pub from_len: u32,
  pub to: *mut SockAddr,
  pub to_len: u32,
}
unsafe extern "C" {
  pub fn round_trip_platform_info(value: PlatformInfo) -> PlatformInfo;
  pub fn receive_packet(info: *const RecvInfo) -> i32;
}
"#,
        )
        .unwrap();
        let bindings = Builder::new()
            .input_bindgen_file(dir.join("bindings.rs"))
            .c_stub_file_header("#include \"library.h\"")
            .generate()
            .unwrap();
        let stub = dir.join("ffi_stub.c");
        bindings.write_c_stub_to_file(&stub).unwrap();

        let rustc = std::process::Command::new("rustc")
            .arg("-vV")
            .output()
            .unwrap();
        let version = String::from_utf8(rustc.stdout).unwrap();
        let target = version
            .lines()
            .find_map(|line| line.strip_prefix("host: "))
            .unwrap();

        cc::Build::new()
            .file(&stub)
            .include(&dir)
            .out_dir(&dir)
            .host(target)
            .target(target)
            .opt_level(0)
            .cargo_metadata(false)
            .warnings_into_errors(true)
            .compile("native_layout_stub");
        let _ = fs::remove_dir_all(dir);
    }
}
