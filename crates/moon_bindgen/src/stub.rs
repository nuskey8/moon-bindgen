use crate::model::{Diagnostic, DiagnosticLevel, Function, Model, Struct, Type};
use crate::{Ownership, Visibility};
use heck::ToSnakeCase;
use std::collections::{BTreeMap, BTreeSet};

pub(crate) struct StubOutput {
    pub moonbit_types: String,
    pub moonbit_functions: String,
    pub c_source: String,
    pub wrapped_symbols: BTreeSet<String>,
    pub value_structs: BTreeSet<String>,
    pub diagnostics: Vec<Diagnostic>,
}

struct Context<'a> {
    model: &'a Model,
    visibility: Visibility,
    ownership_resolver: &'a dyn Fn(&str, &str) -> Ownership,
    function_rename: fn(String) -> String,
    type_rename: fn(String) -> String,
}

#[derive(Clone)]
struct Leaf {
    path: Vec<String>,
    ty: Type,
}

pub(crate) fn render(
    model: &Model,
    visibility: Visibility,
    function_filter: fn(String) -> bool,
    type_filter: fn(String) -> bool,
    ownership_resolver: &dyn Fn(&str, &str) -> Ownership,
    function_rename: fn(String) -> String,
    type_rename: fn(String) -> String,
) -> StubOutput {
    let ctx = Context {
        model,
        visibility,
        ownership_resolver,
        function_rename,
        type_rename,
    };
    let mut candidates = BTreeSet::new();
    for function in model.functions.values() {
        if !function_filter(function.rust_name.clone()) {
            continue;
        }
        for (_, ty) in &function.params {
            if let Some(name) = by_value_struct(ty, model) {
                candidates.insert(name.to_owned());
            }
        }
        if let Some(name) = by_value_struct(&function.result, model) {
            candidates.insert(name.to_owned());
        }
    }

    let roots = candidates
        .into_iter()
        .filter(|name| {
            type_filter(name.clone())
                && struct_supported(name, model, &mut vec![])
                && struct_dependencies_allowed(name, model, type_filter, &mut vec![])
        })
        .collect::<Vec<_>>();
    let mut value_structs = BTreeSet::new();
    for name in roots {
        collect_struct_dependencies(&name, model, &mut value_structs);
    }
    let mut moonbit_types = String::new();
    for name in &value_structs {
        emit_moonbit_struct(&mut moonbit_types, &model.structs[name], &ctx);
    }

    let constructible_structs = model
        .structs
        .keys()
        .filter(|name| {
            !value_structs.contains(*name)
                && type_filter((*name).clone())
                && struct_supported(name, model, &mut vec![])
                && struct_dependencies_allowed(name, model, type_filter, &mut vec![])
        })
        .cloned()
        .collect::<BTreeSet<_>>();

    let mut wrapped_symbols = BTreeSet::new();
    let mut diagnostics = Vec::new();
    let mut moonbit_functions = String::new();
    let mut c_functions = String::new();
    let mut result_structs = BTreeSet::new();
    for function in model.functions.values() {
        if !function_filter(function.rust_name.clone()) || function.variadic {
            continue;
        }
        let needs_stub = function
            .params
            .iter()
            .any(|(_, ty)| by_value_struct(ty, model).is_some())
            || by_value_struct(&function.result, model).is_some();
        if !needs_stub {
            continue;
        }
        if !wrapper_supported(function, &value_structs, model) {
            diagnostics.push(warning(
                &function.rust_name,
                "signature contains a struct value that cannot be marshalled by the generated C stub",
            ));
            continue;
        }
        if let Some(name) = by_value_struct(&function.result, model) {
            result_structs.insert(name.to_owned());
        }
        emit_wrapper(&mut moonbit_functions, &mut c_functions, function, &ctx);
        wrapped_symbols.insert(function.symbol.clone());
    }

    let mut c_types = String::new();
    for structure in model.structs.values() {
        c_types.push_str(&format!(
            "typedef {} moon_bindgen_c_{} moon_bindgen_c_{};\n",
            if structure.is_union {
                "union"
            } else {
                "struct"
            },
            structure.name,
            structure.name
        ));
    }
    if !model.structs.is_empty() {
        c_types.push('\n');
    }
    let mut emitted = BTreeSet::new();
    for name in value_structs.iter().chain(&constructible_structs) {
        emit_c_struct(name, model, &mut emitted, &mut c_types);
    }
    let mut constructors_moon = String::new();
    let mut constructors_c = String::new();
    for name in &constructible_structs {
        emit_constructor(name, &ctx, &mut constructors_moon, &mut constructors_c);
    }
    let mut getters_moon = String::new();
    let mut getters_c = String::new();
    for name in result_structs {
        emit_result_helpers(&name, &ctx, &mut getters_moon, &mut getters_c);
    }
    moonbit_functions = format!("{constructors_moon}{getters_moon}{moonbit_functions}");
    let c_source = if wrapped_symbols.is_empty() && constructible_structs.is_empty() {
        String::new()
    } else {
        format!(
            "// Generated by moon-bindgen. Do not edit.\n#include <moonbit.h>\n#include <stdbool.h>\n#include <stdint.h>\n#include <string.h>\n\n{c_types}{constructors_c}{getters_c}{c_functions}"
        )
    };
    StubOutput {
        moonbit_types,
        moonbit_functions,
        c_source,
        wrapped_symbols,
        value_structs,
        diagnostics,
    }
}

fn emit_constructor(name: &str, ctx: &Context<'_>, moon: &mut String, c: &mut String) {
    let ty = type_name(name, ctx.type_rename);
    let symbol = format!("moon_bindgen_{}_new", name.to_snake_case());
    let leaves = struct_leaves(name, ctx.model);
    let mut params = Vec::new();
    let mut annotations = Vec::new();
    let mut validations = Vec::new();
    let mut c_params = Vec::new();
    let mut assignments = String::new();

    for leaf in &leaves {
        let param = value_name(&leaf.path.join("_"));
        let moon_ty = moon_type(&leaf.ty, ctx.model, ctx.type_rename).unwrap();
        let is_array = matches!(resolve_alias(&leaf.ty, ctx.model), Type::Array { .. });
        params.push(format!("{param} : {moon_ty}"));
        if is_array || needs_ownership(&leaf.ty) {
            annotations.push((
                param.clone(),
                moon_ty.clone(),
                is_array,
                Some(Ownership::Borrow),
            ));
        }
        if let Type::Array { len: Some(len), .. } = resolve_alias(&leaf.ty, ctx.model) {
            validations.push(format!(
                "  if {param}.length() != {len} {{ abort(\"{param} must contain exactly {len} elements\") }}\n"
            ));
        }
        c_params.push(format!(
            "{} {param}",
            c_abi_type(&leaf.ty, ctx.model).unwrap()
        ));
        let target = format!("value->{}", leaf.path.join("."));
        if is_array {
            assignments.push_str(&format!("  memcpy({target}, {param}, sizeof({target}));\n"));
        } else {
            assignments.push_str(&format!("  {target} = {param};\n"));
        }
    }

    emit_annotations(moon, &annotations);
    moon.push_str(&format!(
        "extern \"c\" fn {symbol}({}) -> {ty} = \"{symbol}\"\n\n",
        params.join(", ")
    ));
    moon.push_str(&format!(
        "///|\n{}fn {ty}::new({}) -> {ty} {{\n",
        ctx.visibility.prefix(),
        params.join(", ")
    ));
    for validation in validations {
        moon.push_str(&validation);
    }
    moon.push_str(&format!(
        "  {symbol}({})\n}}\n\n",
        params
            .iter()
            .map(|param| param.split_once(" : ").unwrap().0)
            .collect::<Vec<_>>()
            .join(", ")
    ));

    c.push_str(&format!(
        "MOONBIT_FFI_EXPORT\nvoid *{symbol}({}) {{\n  moon_bindgen_c_{name} *value = (moon_bindgen_c_{name} *)moonbit_make_external_object(NULL, sizeof(moon_bindgen_c_{name}));\n{assignments}  return value;\n}}\n\n",
        c_params.join(", ")
    ));

    for leaf in &leaves {
        emit_field_accessors(name, &ty, leaf, ctx, moon, c);
    }
}

fn emit_field_accessors(
    struct_name: &str,
    struct_ty: &str,
    leaf: &Leaf,
    ctx: &Context<'_>,
    moon: &mut String,
    c: &mut String,
) {
    let field = value_name(&leaf.path.join("_"));
    let field_expr = leaf.path.join(".");
    let moon_ty = moon_type(&leaf.ty, ctx.model, ctx.type_rename).unwrap();
    let prefix = format!("moon_bindgen_{}_{}", struct_name.to_snake_case(), field);
    let getter = format!("{prefix}_get");
    let setter = format!("{prefix}_set");

    if let Type::Array {
        inner,
        len: Some(len),
    } = resolve_alias(&leaf.ty, ctx.model)
    {
        let initial = default_value(inner, ctx.model).unwrap();
        moon.push_str(&format!(
            "#borrow(object, out)\nextern \"c\" fn {getter}(object : {struct_ty}, out : {moon_ty}) = \"{getter}\"\n\n"
        ));
        moon.push_str(&format!(
            "///|\n{}fn {struct_ty}::get_{field}(self : {struct_ty}) -> {moon_ty} {{\n  let out = FixedArray::make({len}, {initial})\n  {getter}(self, out)\n  out\n}}\n\n",
            ctx.visibility.prefix()
        ));
        moon.push_str(&format!(
            "#borrow(object, value)\nextern \"c\" fn {setter}(object : {struct_ty}, value : {moon_ty}) = \"{setter}\"\n\n"
        ));
        moon.push_str(&format!(
            "///|\n{}fn {struct_ty}::set_{field}(self : {struct_ty}, value : {moon_ty}) -> Unit {{\n  if value.length() != {len} {{ abort(\"value must contain exactly {len} elements\") }}\n  {setter}(self, value)\n}}\n\n",
            ctx.visibility.prefix()
        ));
        c.push_str(&format!(
            "MOONBIT_FFI_EXPORT\nvoid {getter}(void *self, void *out) {{\n  moon_bindgen_c_{struct_name} *value = (moon_bindgen_c_{struct_name} *)self;\n  memcpy(out, value->{field_expr}, sizeof(value->{field_expr}));\n}}\n\n"
        ));
        c.push_str(&format!(
            "MOONBIT_FFI_EXPORT\nvoid {setter}(void *self, void *field) {{\n  moon_bindgen_c_{struct_name} *value = (moon_bindgen_c_{struct_name} *)self;\n  memcpy(value->{field_expr}, field, sizeof(value->{field_expr}));\n}}\n\n"
        ));
        return;
    }

    moon.push_str(&format!(
        "#borrow(object)\nextern \"c\" fn {getter}(object : {struct_ty}) -> {moon_ty} = \"{getter}\"\n\n"
    ));
    moon.push_str(&format!(
        "///|\n{}fn {struct_ty}::get_{field}(self : {struct_ty}) -> {moon_ty} {{\n  {getter}(self)\n}}\n\n",
        ctx.visibility.prefix()
    ));
    let setter_borrow = if needs_ownership(&leaf.ty) {
        "#borrow(object, value)"
    } else {
        "#borrow(object)"
    };
    moon.push_str(&format!(
        "{setter_borrow}\nextern \"c\" fn {setter}(object : {struct_ty}, value : {moon_ty}) = \"{setter}\"\n\n"
    ));
    moon.push_str(&format!(
        "///|\n{}fn {struct_ty}::set_{field}(self : {struct_ty}, value : {moon_ty}) -> Unit {{\n  {setter}(self, value)\n}}\n\n",
        ctx.visibility.prefix()
    ));
    let c_ty = c_abi_type(&leaf.ty, ctx.model).unwrap();
    let return_value = if matches!(resolve_alias(&leaf.ty, ctx.model), Type::Pointer { .. }) {
        format!("({c_ty})value->{field_expr}")
    } else {
        format!("value->{field_expr}")
    };
    c.push_str(&format!(
        "MOONBIT_FFI_EXPORT\n{c_ty} {getter}(void *self) {{\n  moon_bindgen_c_{struct_name} *value = (moon_bindgen_c_{struct_name} *)self;\n  return {return_value};\n}}\n\n"
    ));
    c.push_str(&format!(
        "MOONBIT_FFI_EXPORT\nvoid {setter}(void *self, {c_ty} field) {{\n  moon_bindgen_c_{struct_name} *value = (moon_bindgen_c_{struct_name} *)self;\n  value->{field_expr} = field;\n}}\n\n"
    ));
}

fn emit_moonbit_struct(out: &mut String, structure: &Struct, ctx: &Context<'_>) {
    let ty = type_name(&structure.name, ctx.type_rename);
    out.push_str(&format!(
        "///|\n{}struct {} {{\n",
        match ctx.visibility {
            Visibility::Private => "",
            Visibility::Public => "pub(all) ",
        },
        ty
    ));
    for field in &structure.fields {
        let ty = moon_type(&field.ty, ctx.model, ctx.type_rename).unwrap();
        out.push_str(&format!("  {} : {}\n", value_name(&field.name), ty));
    }
    out.push_str("}\n\n");
    let params = structure
        .fields
        .iter()
        .map(|field| {
            format!(
                "{} : {}",
                value_name(&field.name),
                moon_type(&field.ty, ctx.model, ctx.type_rename).unwrap()
            )
        })
        .collect::<Vec<_>>();
    out.push_str(&format!(
        "///|\n{}fn {ty}::new({}) -> {ty} {{\n  {ty}::{{ {} }}\n}}\n\n",
        ctx.visibility.prefix(),
        params.join(", "),
        structure
            .fields
            .iter()
            .map(|field| value_name(&field.name))
            .collect::<Vec<_>>()
            .join(", ")
    ));
}

fn emit_wrapper(moon: &mut String, c: &mut String, function: &Function, ctx: &Context<'_>) {
    let shim = shim_name(&function.symbol);
    let mut shim_params = Vec::<(String, String, bool, Option<Ownership>)>::new();
    let mut wrapper_params = Vec::new();
    let mut validations = Vec::new();
    let mut call_args = Vec::new();
    let mut c_params = Vec::new();
    let mut c_locals = String::new();
    let mut c_call_args = Vec::new();

    for (param_name, ty) in &function.params {
        let safe_param = value_name(param_name);
        wrapper_params.push(format!(
            "{safe_param} : {}",
            moon_type(ty, ctx.model, ctx.type_rename).unwrap()
        ));
        if let Some(struct_name) = by_value_struct(ty, ctx.model) {
            let local = format!("moon_bindgen_arg_{safe_param}");
            c_locals.push_str(&format!("  moon_bindgen_c_{struct_name} {local};\n"));
            let leaves = struct_leaves(struct_name, ctx.model);
            for leaf in leaves {
                let flat = format!("{}_{}", safe_param, leaf.path.join("_"));
                let moon_ty = moon_type(&leaf.ty, ctx.model, ctx.type_rename).unwrap();
                let is_array = matches!(leaf.ty, Type::Array { .. });
                let ownership = if needs_ownership(&leaf.ty) && !is_array {
                    Some((ctx.ownership_resolver)(&function.rust_name, param_name))
                } else {
                    None
                };
                shim_params.push((flat.clone(), moon_ty, is_array, ownership));
                call_args.push(format!("{safe_param}.{}", leaf.path.join(".")));
                if let Type::Array { len: Some(len), .. } = leaf.ty {
                    let expression = format!("{safe_param}.{}", leaf.path.join("."));
                    validations.push(format!(
                        "  if {expression}.length() != {len} {{ abort(\"{expression} must contain exactly {len} elements\") }}\n"
                    ));
                }
                c_params.push(format!(
                    "{} {flat}",
                    c_abi_type(&leaf.ty, ctx.model).unwrap()
                ));
                let target = format!("{local}.{}", leaf.path.join("."));
                if is_array {
                    c_locals.push_str(&format!("  memcpy({target}, {flat}, sizeof({target}));\n"));
                } else {
                    c_locals.push_str(&format!("  {target} = {flat};\n"));
                }
            }
            c_call_args.push(local);
        } else {
            let moon_ty = moon_type(ty, ctx.model, ctx.type_rename).unwrap();
            let ownership = if needs_ownership(ty) {
                Some((ctx.ownership_resolver)(&function.rust_name, param_name))
            } else {
                None
            };
            shim_params.push((safe_param.clone(), moon_ty, false, ownership));
            call_args.push(safe_param.clone());
            c_params.push(format!(
                "{} {safe_param}",
                c_abi_type(ty, ctx.model).unwrap()
            ));
            c_call_args.push(c_call_expr(&safe_param, ty, ctx.model));
        }
    }

    let result_struct = by_value_struct(&function.result, ctx.model);
    let shim_result = if let Some(name) = result_struct {
        repr_name(name, ctx.type_rename)
    } else {
        moon_type(&function.result, ctx.model, ctx.type_rename).unwrap()
    };
    emit_annotations(moon, &shim_params);
    moon.push_str(&format!("extern \"c\" fn {shim}("));
    moon.push_str(
        &shim_params
            .iter()
            .map(|(name, ty, _, _)| format!("{name} : {ty}"))
            .collect::<Vec<_>>()
            .join(", "),
    );
    moon.push(')');
    if shim_result != "Unit" {
        moon.push_str(&format!(" -> {shim_result}"));
    }
    moon.push_str(&format!(" = \"{shim}\"\n\n"));

    moon.push_str(&format!(
        "///|\n{}fn {}({})",
        ctx.visibility.prefix(),
        safe_ident(&(ctx.function_rename)(function.rust_name.clone())),
        wrapper_params.join(", ")
    ));
    let public_result = moon_type(&function.result, ctx.model, ctx.type_rename).unwrap();
    if public_result != "Unit" {
        moon.push_str(&format!(" -> {public_result}"));
    }
    moon.push_str(" {\n");
    for validation in validations {
        moon.push_str(&validation);
    }
    if let Some(name) = result_struct {
        moon.push_str(&format!(
            "  let result = {shim}({})\n",
            call_args.join(", ")
        ));
        moon.push_str("  ");
        moon.push_str(&construct_struct(name, name, &[], "result", ctx, 1));
        moon.push('\n');
    } else {
        moon.push_str(&format!("  {shim}({})\n", call_args.join(", ")));
    }
    moon.push_str("}\n\n");

    let original_result = c_type(&function.result, ctx.model).unwrap();
    c.push_str(&format!(
        "extern {original_result} {}({});\n",
        function.symbol,
        function
            .params
            .iter()
            .map(|(name, ty)| c_decl(ty, name, ctx.model).unwrap())
            .collect::<Vec<_>>()
            .join(", ")
    ));
    let wrapper_result = if result_struct.is_some() {
        "moonbit_bytes_t".to_owned()
    } else {
        c_abi_type(&function.result, ctx.model).unwrap()
    };
    c.push_str(&format!(
        "MOONBIT_FFI_EXPORT\n{wrapper_result} {shim}({}) {{\n{c_locals}",
        c_params.join(", ")
    ));
    let call = format!("{}({})", function.symbol, c_call_args.join(", "));
    if let Some(name) = result_struct {
        c.push_str(&format!(
            "  moon_bindgen_c_{name} result = {call};\n  moonbit_bytes_t bytes = moonbit_make_bytes((int32_t)sizeof(result), 0);\n  memcpy(bytes, &result, sizeof(result));\n  return bytes;\n"
        ));
    } else if matches!(resolve_alias(&function.result, ctx.model), Type::Unit) {
        c.push_str(&format!("  {call};\n"));
    } else {
        c.push_str(&format!("  return {call};\n"));
    }
    c.push_str("}\n\n");
}

fn emit_result_helpers(name: &str, ctx: &Context<'_>, moon: &mut String, c: &mut String) {
    let repr = repr_name(name, ctx.type_rename);
    moon.push_str(&format!("///|\npriv struct {repr}(Bytes)\n\n"));
    for leaf in struct_leaves(name, ctx.model) {
        let suffix = leaf.path.join("_").to_snake_case();
        let getter = format!("moon_bindgen_get_{}_{}", name.to_snake_case(), suffix);
        let field_expr = leaf.path.join(".");
        let moon_ty = moon_type(&leaf.ty, ctx.model, ctx.type_rename).unwrap();
        if let Type::Array { len: Some(len), .. } = leaf.ty {
            moon.push_str(&format!(
                "///|\n#borrow(value, out)\nextern \"c\" fn {getter}(value : {repr}, out : {moon_ty}) = \"{getter}\"\n\n"
            ));
            c.push_str(&format!(
                "MOONBIT_FFI_EXPORT\nvoid {getter}(moonbit_bytes_t value, void *out) {{\n  moon_bindgen_c_{name} *typed = (moon_bindgen_c_{name} *)value;\n  memcpy(out, typed->{field_expr}, sizeof(typed->{field_expr}));\n}}\n\n"
            ));
            let _ = len;
        } else {
            moon.push_str(&format!(
                "///|\n#borrow(value)\nextern \"c\" fn {getter}(value : {repr}) -> {moon_ty} = \"{getter}\"\n\n"
            ));
            c.push_str(&format!(
                "MOONBIT_FFI_EXPORT\n{} {getter}(moonbit_bytes_t value) {{\n  moon_bindgen_c_{name} *typed = (moon_bindgen_c_{name} *)value;\n  return typed->{field_expr};\n}}\n\n",
                c_abi_type(&leaf.ty, ctx.model).unwrap()
            ));
        }
    }
}

fn construct_struct(
    name: &str,
    root_name: &str,
    prefix: &[String],
    repr_var: &str,
    ctx: &Context<'_>,
    indent: usize,
) -> String {
    let structure = &ctx.model.structs[name];
    let mut out = format!("{}::{{\n", type_name(name, ctx.type_rename));
    for field in &structure.fields {
        let field_name = value_name(&field.name);
        let mut field_path = prefix.to_vec();
        field_path.push(field.name.clone());
        out.push_str(&"  ".repeat(indent + 1));
        out.push_str(&format!("{field_name}: "));
        if let Some(nested) = by_value_struct(&field.ty, ctx.model) {
            out.push_str(&construct_struct(
                nested,
                root_name,
                &field_path,
                repr_var,
                ctx,
                indent + 1,
            ));
        } else {
            let getter = format!(
                "moon_bindgen_get_{}_{}",
                root_name.to_snake_case(),
                field_path.join("_").to_snake_case()
            );
            if let Type::Array {
                inner,
                len: Some(len),
            } = resolve_alias(&field.ty, ctx.model)
            {
                let initial = default_value(inner, ctx.model).unwrap();
                out.push_str(&format!(
                    "{{ let out = FixedArray::make({len}, {initial}); {getter}({repr_var}, out); out }}"
                ));
            } else {
                out.push_str(&format!("{getter}({repr_var})"));
            }
        }
        out.push_str(",\n");
    }
    out.push_str(&"  ".repeat(indent));
    out.push('}');
    out
}

fn emit_annotations(out: &mut String, params: &[(String, String, bool, Option<Ownership>)]) {
    let borrowed = params
        .iter()
        .filter(|(_, _, array, ownership)| *array || *ownership == Some(Ownership::Borrow))
        .map(|(name, _, _, _)| name.as_str())
        .collect::<Vec<_>>();
    if !borrowed.is_empty() {
        out.push_str(&format!("#borrow({})\n", borrowed.join(", ")));
    }
    let owned = params
        .iter()
        .filter(|(_, _, _, ownership)| *ownership == Some(Ownership::Owned))
        .map(|(name, _, _, _)| name.as_str())
        .collect::<Vec<_>>();
    if !owned.is_empty() {
        out.push_str(&format!("#owned({})\n", owned.join(", ")));
    }
}

fn emit_c_struct(name: &str, model: &Model, emitted: &mut BTreeSet<String>, out: &mut String) {
    if emitted.contains(name) {
        return;
    }
    let structure = &model.structs[name];
    for field in &structure.fields {
        if let Some(nested) = by_value_struct(&field.ty, model) {
            emit_c_struct(nested, model, emitted, out);
        }
    }
    out.push_str(&format!("struct moon_bindgen_c_{name} {{\n"));
    for field in &structure.fields {
        out.push_str("  ");
        out.push_str(&c_decl(&field.ty, &field.name, model).unwrap());
        out.push_str(";\n");
    }
    out.push_str("};\n\n");
    emitted.insert(name.to_owned());
}

fn wrapper_supported(function: &Function, values: &BTreeSet<String>, model: &Model) -> bool {
    if function.variadic {
        return false;
    }
    function
        .params
        .iter()
        .all(|(_, ty)| wrapper_type_supported(ty, values, model))
        && wrapper_type_supported(&function.result, values, model)
        && by_value_struct(&function.result, model)
            .is_none_or(|name| struct_result_supported(name, model, &mut vec![]))
}

fn struct_result_supported(name: &str, model: &Model, stack: &mut Vec<String>) -> bool {
    if stack.iter().any(|item| item == name) {
        return false;
    }
    stack.push(name.to_owned());
    let supported =
        model.structs[name]
            .fields
            .iter()
            .all(|field| match resolve_alias(&field.ty, model) {
                Type::Pointer { .. } | Type::FunctionPointer { .. } => false,
                Type::Path(path) if model.structs.contains_key(last(path)) => {
                    struct_result_supported(last(path), model, stack)
                }
                _ => true,
            });
    stack.pop();
    supported
}

fn struct_dependencies_allowed(
    name: &str,
    model: &Model,
    type_filter: fn(String) -> bool,
    stack: &mut Vec<String>,
) -> bool {
    if stack.iter().any(|item| item == name) || !type_filter(name.to_owned()) {
        return false;
    }
    stack.push(name.to_owned());
    let allowed = model.structs[name].fields.iter().all(|field| {
        by_value_struct(&field.ty, model)
            .is_none_or(|nested| struct_dependencies_allowed(nested, model, type_filter, stack))
    });
    stack.pop();
    allowed
}

fn collect_struct_dependencies(name: &str, model: &Model, out: &mut BTreeSet<String>) {
    if !out.insert(name.to_owned()) {
        return;
    }
    for field in &model.structs[name].fields {
        if let Some(nested) = by_value_struct(&field.ty, model) {
            collect_struct_dependencies(nested, model, out);
        }
    }
}

fn wrapper_type_supported(ty: &Type, values: &BTreeSet<String>, model: &Model) -> bool {
    match resolve_alias(ty, model) {
        Type::Unit => true,
        Type::Path(path) => {
            let name = last(path);
            primitive_c_type(name).is_some()
                || (values.contains(name) && struct_supported(name, model, &mut vec![]))
        }
        Type::Pointer { inner, .. } => pointer_pointee_supported(inner, model),
        Type::Array { .. } | Type::FunctionPointer { .. } | Type::Unsupported => false,
    }
}

fn struct_supported(name: &str, model: &Model, stack: &mut Vec<String>) -> bool {
    let Some(structure) = model.structs.get(name) else {
        return false;
    };
    if structure.is_union || structure.fields.is_empty() || stack.iter().any(|item| item == name) {
        return false;
    }
    stack.push(name.to_owned());
    let supported = structure.fields.iter().all(|field| match resolve_alias(&field.ty, model) {
        Type::Path(path) if primitive_c_type(last(path)).is_some() => true,
        Type::Path(path) if model.structs.contains_key(last(path)) => {
            struct_supported(last(path), model, stack)
        }
        Type::Pointer { inner, .. } => pointer_pointee_supported(inner, model),
        Type::Array {
            inner,
            len: Some(len),
        } => {
            *len > 0
                && matches!(resolve_alias(inner, model), Type::Path(path) if primitive_c_type(last(path)).is_some())
        }
        _ => false,
    });
    stack.pop();
    supported
}

fn struct_leaves(name: &str, model: &Model) -> Vec<Leaf> {
    fn visit(name: &str, model: &Model, prefix: &mut Vec<String>, out: &mut Vec<Leaf>) {
        for field in &model.structs[name].fields {
            prefix.push(field.name.clone());
            if let Some(nested) = by_value_struct(&field.ty, model) {
                visit(nested, model, prefix, out);
            } else {
                out.push(Leaf {
                    path: prefix.clone(),
                    ty: field.ty.clone(),
                });
            }
            prefix.pop();
        }
    }
    let mut out = Vec::new();
    visit(name, model, &mut Vec::new(), &mut out);
    out
}

fn by_value_struct<'a>(ty: &'a Type, model: &'a Model) -> Option<&'a str> {
    match resolve_alias(ty, model) {
        Type::Path(path) if model.structs.contains_key(last(path)) => Some(last(path)),
        _ => None,
    }
}

fn resolve_alias<'a>(ty: &'a Type, model: &'a Model) -> &'a Type {
    let mut current = ty;
    let mut seen = BTreeSet::new();
    loop {
        let Type::Path(path) = current else {
            return current;
        };
        let name = last(path);
        if !seen.insert(name) {
            return current;
        }
        let Some(alias) = model.aliases.get(name) else {
            return current;
        };
        current = alias;
    }
}

fn moon_type(ty: &Type, model: &Model, rename: fn(String) -> String) -> Option<String> {
    if let Some(name) = opaque_pointer_carrier_name(ty, model, rename) {
        return Some(name);
    }
    match resolve_alias(ty, model) {
        Type::Unit => Some("Unit".into()),
        Type::Path(path) => Some(match last(path) {
            "i8" | "u8" | "c_char" | "c_schar" | "c_uchar" => "Byte".into(),
            "i16" | "u16" | "c_short" | "c_ushort" | "i32" | "c_int" => "Int".into(),
            "u32" | "c_uint" => "UInt".into(),
            "i64" | "c_longlong" | "isize" | "c_long" => "Int64".into(),
            "u64" | "c_ulonglong" | "usize" | "c_ulong" | "size_t" => "UInt64".into(),
            "f32" | "c_float" => "Float".into(),
            "f64" | "c_double" => "Double".into(),
            "bool" => "Bool".into(),
            "c_void" => "Unit".into(),
            other => type_name(other, rename),
        }),
        Type::Pointer { inner, .. } => match resolve_alias(inner, model) {
            Type::Path(path) if matches!(last(path), "c_char" | "i8" | "u8") => {
                Some("Bytes".into())
            }
            Type::Path(path) if last(path) == "c_void" => Some("RawPtr".into()),
            Type::Path(path) if primitive_c_type(last(path)).is_some() => {
                Some(format!("Ref[{}]", moon_type(inner, model, rename)?))
            }
            Type::Path(path) => Some(type_name(last(path), rename)),
            _ => moon_type(inner, model, rename).map(|ty| format!("Ref[{ty}]")),
        },
        Type::Array { inner, .. } => {
            Some(format!("FixedArray[{}]", moon_type(inner, model, rename)?))
        }
        _ => None,
    }
}

fn opaque_pointer_carrier_name(
    ty: &Type,
    model: &Model,
    rename: fn(String) -> String,
) -> Option<String> {
    let mut current = resolve_alias(ty, model);
    let mut depth = 0;
    while let Type::Pointer { inner, .. } = current {
        depth += 1;
        current = resolve_alias(inner, model);
    }
    let Type::Path(path) = current else {
        return None;
    };
    let base = last(path);
    (depth >= 2 && primitive_c_type(base).is_none() && base != "c_void")
        .then(|| format!("{}{}", type_name(base, rename), "Ptr".repeat(depth - 1)))
}

fn c_type(ty: &Type, model: &Model) -> Option<String> {
    match resolve_alias(ty, model) {
        Type::Unit => Some("void".into()),
        Type::Path(path) => primitive_c_type(last(path)).map(str::to_owned).or_else(|| {
            model
                .structs
                .contains_key(last(path))
                .then(|| format!("moon_bindgen_c_{}", last(path)))
        }),
        Type::Pointer { inner, mutable } => {
            let qualifier = if *mutable { "" } else { "const " };
            Some(format!("{qualifier}{} *", c_type(inner, model)?))
        }
        _ => None,
    }
}

fn c_decl(ty: &Type, name: &str, model: &Model) -> Option<String> {
    match resolve_alias(ty, model) {
        Type::Array {
            inner,
            len: Some(len),
        } => Some(format!("{} {name}[{len}]", c_type(inner, model)?)),
        _ => Some(format!("{} {name}", c_type(ty, model)?)),
    }
}

fn c_abi_type(ty: &Type, model: &Model) -> Option<String> {
    match resolve_alias(ty, model) {
        Type::Unit => Some("void".into()),
        Type::Pointer { inner, .. } => match resolve_alias(inner, model) {
            Type::Path(path) if matches!(last(path), "c_char" | "i8" | "u8") => {
                Some("moonbit_bytes_t".into())
            }
            Type::Path(path) if primitive_c_type(last(path)).is_some() => c_type(ty, model),
            Type::Path(_) => Some("void *".into()),
            Type::Pointer { .. } => c_type(ty, model),
            _ => None,
        },
        Type::Array { inner, .. } => Some(format!("{} *", c_type(inner, model)?)),
        _ => c_type(ty, model),
    }
}

fn pointer_pointee_supported(ty: &Type, model: &Model) -> bool {
    match resolve_alias(ty, model) {
        Type::Path(_) => true,
        Type::Pointer { inner, .. } => pointer_pointee_supported(inner, model),
        _ => false,
    }
}

fn c_call_expr(name: &str, ty: &Type, model: &Model) -> String {
    if matches!(resolve_alias(ty, model), Type::Pointer { .. }) {
        format!("({}){name}", c_type(ty, model).unwrap())
    } else {
        name.to_owned()
    }
}

fn primitive_c_type(name: &str) -> Option<&'static str> {
    match name {
        "i8" | "c_schar" | "c_char" => Some("int8_t"),
        "u8" | "c_uchar" => Some("uint8_t"),
        "i16" | "c_short" => Some("int16_t"),
        "u16" | "c_ushort" => Some("uint16_t"),
        "i32" | "c_int" => Some("int32_t"),
        "u32" | "c_uint" => Some("uint32_t"),
        "i64" | "isize" | "c_long" | "c_longlong" => Some("int64_t"),
        "u64" | "usize" | "size_t" | "c_ulong" | "c_ulonglong" => Some("uint64_t"),
        "f32" | "c_float" => Some("float"),
        "f64" | "c_double" => Some("double"),
        "bool" => Some("bool"),
        "c_void" => Some("void"),
        _ => None,
    }
}

fn default_value(ty: &Type, model: &Model) -> Option<&'static str> {
    match resolve_alias(ty, model) {
        Type::Path(path) => match last(path) {
            "i8" | "u8" | "c_char" | "c_schar" | "c_uchar" => Some("Byte::default()"),
            "i16" | "u16" | "c_short" | "c_ushort" | "i32" | "c_int" => Some("Int::default()"),
            "u32" | "c_uint" => Some("UInt::default()"),
            "i64" | "c_longlong" | "isize" | "c_long" => Some("Int64::default()"),
            "u64" | "c_ulonglong" | "usize" | "c_ulong" | "size_t" => Some("UInt64::default()"),
            "f32" | "c_float" => Some("Float::default()"),
            "f64" | "c_double" => Some("Double::default()"),
            "bool" => Some("Bool::default()"),
            _ => None,
        },
        _ => None,
    }
}

fn needs_ownership(ty: &Type) -> bool {
    matches!(ty, Type::Pointer { .. } | Type::Array { .. })
}

fn last(path: &str) -> &str {
    path.rsplit("::").next().unwrap_or(path)
}

fn shim_name(symbol: &str) -> String {
    format!("moon_bindgen_{}", symbol.to_snake_case())
}

fn repr_name(name: &str, rename: fn(String) -> String) -> String {
    format!("{}MoonBindgenRepr", type_name(name, rename))
}

fn type_name(name: &str, rename: fn(String) -> String) -> String {
    safe_ident(&rename(name.to_owned()))
}

fn value_name(name: &str) -> String {
    safe_ident(&name.replace("r#", "").to_snake_case())
}

fn safe_ident(name: &str) -> String {
    if matches!(
        name,
        "type"
            | "fn"
            | "let"
            | "const"
            | "struct"
            | "enum"
            | "match"
            | "if"
            | "else"
            | "loop"
            | "for"
            | "while"
            | "return"
            | "extern"
            | "pub"
            | "priv"
            | "true"
            | "false"
    ) {
        format!("{name}_")
    } else {
        name.to_owned()
    }
}

fn warning(item: &str, message: &str) -> Diagnostic {
    Diagnostic {
        level: DiagnosticLevel::Warning,
        item: item.to_owned(),
        message: message.to_owned(),
    }
}

#[allow(dead_code)]
fn _assert_maps_are_ordered(_: &BTreeMap<String, String>) {}
