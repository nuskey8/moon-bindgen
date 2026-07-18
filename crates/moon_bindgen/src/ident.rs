pub(crate) fn safe_moonbit_ident(name: &str) -> String {
    let name = name.replace("r#", "");
    if is_moonbit_keyword(&name) {
        format!("{name}_")
    } else {
        name
    }
}

fn is_moonbit_keyword(name: &str) -> bool {
    matches!(
        name,
        // Keywords documented by the MoonBit language reference.
        "as" | "else" | "extern" | "fn" | "fnalias" | "if" | "let" | "const"
            | "match" | "using" | "mut" | "type" | "typealias" | "struct" | "enum"
            | "extenum" | "trait" | "traitalias" | "derive" | "while" | "break"
            | "continue" | "import" | "return" | "throw" | "raise" | "try" | "catch"
            | "pub" | "priv" | "proof_assert" | "proof_let" | "readonly" | "true"
            | "false" | "_" | "test" | "loop" | "for" | "in" | "impl" | "with"
            | "guard" | "async" | "is" | "suberror" | "and" | "letrec" | "enumview"
            | "noraise" | "defer" | "lexmatch" | "where" | "declare" | "nobreak"
            // Reserved words currently accepted with a warning and intended for future use.
            | "module" | "move" | "ref" | "static" | "super" | "unsafe" | "use"
            | "await" | "dyn" | "abstract" | "do" | "final" | "macro" | "override"
            | "typeof" | "virtual" | "yield" | "local" | "method" | "alias" | "assert"
            | "package" | "recur" | "isnot" | "define" | "downcast" | "inherit"
            | "member" | "namespace" | "upcast" | "void" | "lazy" | "include"
            | "mixin" | "protected" | "sealed" | "constructor" | "atomic" | "volatile"
            | "anyframe" | "anytype" | "asm" | "comptime" | "errdefer" | "export"
            | "opaque" | "orelse" | "resume" | "threadlocal" | "unreachable" | "dynclass"
            | "dynobj" | "dynrec" | "var" | "finally" | "noasync" | "assume"
    )
}

#[cfg(test)]
mod tests {
    use super::safe_moonbit_ident;

    #[test]
    fn escapes_current_and_reserved_keywords() {
        assert_eq!(safe_moonbit_ident("r#type"), "type_");
        assert_eq!(safe_moonbit_ident("guard"), "guard_");
        assert_eq!(safe_moonbit_ident("module"), "module_");
        assert_eq!(safe_moonbit_ident("opaque"), "opaque_");
        assert_eq!(safe_moonbit_ident("ordinary_name"), "ordinary_name");
    }
}
