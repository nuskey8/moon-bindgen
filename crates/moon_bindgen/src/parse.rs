use crate::model::{Constant, Field, Function, Model, Struct, Type};
use syn::{Attribute, Expr, ForeignItem, Item, Lit, Meta, ReturnType, Type as SynType};

pub(crate) fn collect_file(file: &syn::File, model: &mut Model) {
    collect_items(&file.items, model);
}
fn collect_items(items: &[Item], model: &mut Model) {
    for item in items {
        match item {
            Item::ForeignMod(m) if is_c_abi(&Some(m.abi.clone())) => {
                for item in &m.items {
                    if let ForeignItem::Fn(f) = item {
                        collect_fn(&f.sig, &f.attrs, model);
                    }
                }
            }
            Item::Fn(f) if is_c_abi(&f.sig.abi) => collect_fn(&f.sig, &f.attrs, model),
            Item::Struct(s) if has_repr_c(&s.attrs) => {
                let fields = collect_fields(&s.fields);
                model.structs.insert(
                    s.ident.to_string(),
                    Struct {
                        name: s.ident.to_string(),
                        is_union: false,
                        is_opaque: fields
                            .iter()
                            .any(|field| is_bindgen_opaque_field(&field.name)),
                        fields,
                    },
                );
            }
            Item::Union(s) if has_repr_c(&s.attrs) => {
                let fields = collect_fields(&syn::Fields::Named(s.fields.clone()));
                model.structs.insert(
                    s.ident.to_string(),
                    Struct {
                        name: s.ident.to_string(),
                        is_union: true,
                        is_opaque: fields
                            .iter()
                            .any(|field| is_bindgen_opaque_field(&field.name)),
                        fields,
                    },
                );
            }
            Item::Type(t) => {
                model.aliases.insert(t.ident.to_string(), parse_type(&t.ty));
            }
            Item::Const(c) => {
                if let Some(value) = expr_text(&c.expr) {
                    model.constants.insert(
                        c.ident.to_string(),
                        Constant {
                            ty: parse_type(&c.ty),
                            value,
                        },
                    );
                }
            }
            Item::Mod(m) => {
                if let Some((_, nested)) = &m.content {
                    collect_items(nested, model);
                }
            }
            _ => {}
        }
    }
}
fn collect_fn(sig: &syn::Signature, attrs: &[Attribute], model: &mut Model) {
    let rust_name = sig.ident.to_string();
    let symbol = export_name(attrs).unwrap_or_else(|| rust_name.clone());
    let mut params = vec![];
    for (i, arg) in sig.inputs.iter().enumerate() {
        if let syn::FnArg::Typed(arg) = arg {
            let name = match arg.pat.as_ref() {
                syn::Pat::Ident(p) => p.ident.to_string(),
                _ => format!("arg{i}"),
            };
            params.push((name, parse_type(&arg.ty)));
        }
    }
    let result = match &sig.output {
        ReturnType::Default => Type::Unit,
        ReturnType::Type(_, t) => parse_type(t),
    };
    model.functions.insert(
        symbol.clone(),
        Function {
            rust_name,
            symbol,
            params,
            result,
            variadic: sig.variadic.is_some(),
        },
    );
}
fn parse_type(ty: &SynType) -> Type {
    match ty {
        SynType::Tuple(t) if t.elems.is_empty() => Type::Unit,
        SynType::Path(p) => {
            if let Some(last) = p.path.segments.last()
                && last.ident == "Option"
                && let syn::PathArguments::AngleBracketed(args) = &last.arguments
                && let Some(syn::GenericArgument::Type(inner)) = args.args.first()
                && let Type::FunctionPointer {
                    params,
                    result,
                    variadic,
                    ..
                } = parse_type(inner)
            {
                return Type::FunctionPointer {
                    params,
                    result,
                    variadic,
                    nullable: true,
                };
            }
            if p.path.segments.last().is_some_and(|segment| {
                segment.ident == "Option" && !matches!(segment.arguments, syn::PathArguments::None)
            }) {
                return Type::Unsupported;
            }
            Type::Path(
                p.path
                    .segments
                    .iter()
                    .map(|s| s.ident.to_string())
                    .collect::<Vec<_>>()
                    .join("::"),
            )
        }
        SynType::Ptr(p) => Type::Pointer {
            inner: Box::new(parse_type(&p.elem)),
            mutable: p.mutability.is_some(),
        },
        SynType::Array(a) => Type::Array {
            inner: Box::new(parse_type(&a.elem)),
            len: array_len(&a.len),
        },
        SynType::BareFn(f) if is_c_abi(&f.abi) => Type::FunctionPointer {
            params: f.inputs.iter().map(|arg| parse_type(&arg.ty)).collect(),
            result: Box::new(match &f.output {
                ReturnType::Default => Type::Unit,
                ReturnType::Type(_, ty) => parse_type(ty),
            }),
            variadic: f.variadic.is_some(),
            nullable: false,
        },
        _ => Type::Unsupported,
    }
}
fn collect_fields(fields: &syn::Fields) -> Vec<Field> {
    fields
        .iter()
        .enumerate()
        .map(|(index, field)| Field {
            name: field
                .ident
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_else(|| format!("_{index}")),
            ty: parse_type(&field.ty),
        })
        .collect()
}

fn is_bindgen_opaque_field(name: &str) -> bool {
    matches!(name, "_bindgen_opaque_blob" | "__bindgen_opaque_blob")
}
fn array_len(expr: &Expr) -> Option<usize> {
    if let Expr::Lit(lit) = expr
        && let Lit::Int(value) = &lit.lit
    {
        value.base10_parse().ok()
    } else {
        None
    }
}
fn is_c_abi(abi: &Option<syn::Abi>) -> bool {
    abi.as_ref()
        .and_then(|a| a.name.as_ref())
        .is_some_and(|n| matches!(n.value().as_str(), "C" | "C-unwind"))
}
fn has_repr_c(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|a| {
        a.path().is_ident("repr")
            && a.meta
                .require_list()
                .is_ok_and(|l| l.tokens.to_string().split(',').any(|x| x.trim() == "C"))
    })
}
fn export_name(attrs: &[Attribute]) -> Option<String> {
    for attr in attrs {
        if (attr.path().is_ident("export_name") || attr.path().is_ident("link_name"))
            && let Meta::NameValue(v) = &attr.meta
            && let Expr::Lit(l) = &v.value
            && let Lit::Str(s) = &l.lit
        {
            return Some(s.value());
        }
        if attr.path().is_ident("unsafe")
            && let Ok(Meta::NameValue(v)) = attr.parse_args::<Meta>()
            && v.path.is_ident("export_name")
            && let Expr::Lit(l) = v.value
            && let Lit::Str(s) = l.lit
        {
            return Some(s.value());
        }
    }
    None
}
fn expr_text(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Lit(l) => match &l.lit {
            Lit::Int(v) => Some(v.base10_digits().into()),
            Lit::Float(v) => Some(v.base10_digits().into()),
            Lit::Bool(v) => Some(v.value.to_string()),
            _ => None,
        },
        Expr::Unary(u) => expr_text(&u.expr).map(|v| {
            format!(
                "{}{}",
                if matches!(u.op, syn::UnOp::Neg(_)) {
                    "-"
                } else {
                    ""
                },
                v
            )
        }),
        _ => None,
    }
}
