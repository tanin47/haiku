use inkwell::AddressSpace;
use inkwell::IntPredicate;
use inkwell::basic_block::BasicBlock;
use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::execution_engine::ExecutionEngine;
use inkwell::module::Linkage;
use inkwell::module::Module;
use inkwell::OptimizationLevel;
use inkwell::targets::{CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetMachine};
use inkwell::types::ArrayType;
use inkwell::types::BasicTypeEnum;
use inkwell::types::StructType;
use inkwell::values::{BasicValue, FunctionValue, IntValue};
use inkwell::values::BasicValueEnum;
use inkwell::values::PointerValue;
use inkwell::values::StructValue;
use inkwell::values::VectorValue;

use semantics::tree;
use inkwell::types::PointerType;

enum Value {
    Void,
    Number(IntValue),
    Boolean(IntValue),
    String(PointerValue),
    Class(PointerValue, *const tree::ClassInstance),
}

fn convert(value: Value) -> BasicValueEnum {
    match value {
        Value::Number(i) => i.into(),
        Value::Boolean(b) => b.into(),
        Value::String(p) => p.into(),
        Value::Class(p, c) => p.into(),
        Value::Void => panic!("can't convert void"),
    }
}

struct Core {
    string_struct_type: StructType
}

struct ModContext<'a, 'b, 'c, 'd> {
    module: &'a Module,
    context: &'b Context,
    builder: &'c Builder,
    core: &'d Core,
}

struct FnContext<'a, 'b, 'c, 'd, 'e> {
    func: &'a FunctionValue,
    module: &'b Module,
    context: &'c Context,
    builder: &'d Builder,
    core: &'e Core,
}

pub fn generate(
    module: &tree::Mod,
    context: &Context,
    builder: &Builder,
) -> Module {
    let llvm_module = context.create_module("main");

    let core = Core {
        string_struct_type: StructType::struct_type(
            &[
                context.i32_type().into(),
                context.i8_type().ptr_type(AddressSpace::Generic).into()
            ],
            false
        ),
    };
    {
        let context = ModContext {
            module: &llvm_module,
            context,
            builder,
            core: &core
        };
        for unit in &module.units {
            gen_mod_unit(unit, &context);
        }
    }
    return llvm_module;
}

fn gen_mod_unit(
    unit: &tree::ModUnit,
    context: &ModContext,
) {
    match unit {
        tree::ModUnit::Func { ref func } => {
            gen_func(func, context);
        },
        _ => (),
    }
}

fn gen_func(
    func: &tree::Func,
    context: &ModContext,
) {
    let i32_type = context.context.i32_type();
    let fn_type = i32_type.fn_type(&[], false);

    let function = context.module.add_function(&func.name, fn_type, None);
    func.llvm_ref.replace(Some(function));

    let first_block = context.context.append_basic_block(&function, "first_block");
    context.builder.position_at_end(&first_block);

    let fn_context = FnContext {
        func: &function,
        module: context.module,
        context: context.context,
        builder: context.builder,
        core: context.core,
    };

    for (index, expr) in func.exprs.iter().enumerate() {
        let ret = gen_expr(expr, &fn_context);
        if index == (func.exprs.len() - 1) {
            context.builder.build_return(Some(&convert(ret)));
        }
    }
}

fn gen_expr(
    expr: &tree::Expr,
    context: &FnContext
) -> Value {
    match expr {
        tree::Expr::Invoke { ref invoke, tpe: _ } => {
            gen_invoke(invoke, context)
        },
        tree::Expr::Num { ref num, tpe: _ } => {
            gen_num(num, context)
        },
        tree::Expr::Assignment { ref assignment, tpe: _ } => {
            gen_assignment(assignment, context)
        },
        tree::Expr::ReadVar { ref read_var, tpe: _ } => {
            gen_read_var(read_var, context)
        },
        tree::Expr::LiteralString { ref literal_string, tpe: _ } => {
            gen_string(literal_string, context)
        },
        tree::Expr::Boolean { ref boolean, tpe: _ } => {
            gen_boolean(boolean, context)
        },
        tree::Expr::Comparison { ref comparison, tpe: _ } => {
            gen_comparison(comparison, context)
        },
        tree::Expr::IfElse { ref if_else, tpe: _ } => {
            gen_if_else(if_else, context)
        },
        tree::Expr::ClassInstance { ref class_instance, tpe: _ } => {
            gen_class_instance(class_instance, context)
        },
    }
}

fn gen_class_instance(
    class_instance: &tree::ClassInstance,
    context: &FnContext
) -> Value {
    let s = match gen_expr(&class_instance.expr, context) {
        Value::String(ptr) => ptr,
        _ => panic!("A class expects one string as its parameter"),
    };
    let class_struct = StructType::struct_type(
        &[
            context.context.i8_type().ptr_type(AddressSpace::Generic).into()
        ],
        false
    );
    let instance= context.builder.build_alloca(class_struct, "class");
    let first_param = unsafe {
        context.builder.build_in_bounds_gep(
            instance,
            &[context.context.i32_type().const_int(0, false), context.context.i32_type().const_int(0, false)],
            "gep")
    };
    context.builder.build_store(first_param, s);

    Value::Class(instance, class_instance as *const tree::ClassInstance)
}

fn gen_boolean(
    boolean: &tree::Boolean,
    context: &FnContext,
) -> Value {
   Value::Boolean(context.context.bool_type().const_int(boolean.value as u64, false))
}

fn gen_comparison(
    comparison: &tree::Comparison,
    context: &FnContext,
) -> Value {
    let var = match gen_read_var(&comparison.left, context) {
        Value::Number(i) => i,
        _ => panic!("Unable to read var into IntValue")
    };
    let num = match gen_num(&comparison.right, context) {
        Value::Number(i) => i,
        _ => panic!("")
    };
    Value::Boolean(context.builder.build_int_compare(IntPredicate::SGT, var, num, "cond"))
}

fn gen_if_else(
    if_else: &tree::IfElse,
    context: &FnContext,
) -> Value {
    let ret_pointer = context.builder.build_alloca(context.context.i32_type().ptr_type(AddressSpace::Generic), "ret_if_else");
    let comparison = match gen_comparison(&if_else.cond, context) {
        Value::Boolean(i) => i,
        _ => panic!(""),
    };
    let true_block = context.context.append_basic_block(context.func, "true_block");
    let false_block = context.context.append_basic_block(context.func, "false_block");
    let end_block = context.context.append_basic_block(context.func, "end");
    context.builder.build_conditional_branch(comparison, &true_block, &false_block);

    context.builder.position_at_end(&true_block);
    let true_value = gen_expr(&if_else.true_br, context);
//    context.builder.build_store(ret_pointer, convert(true_value));
    context.builder.build_unconditional_branch(&end_block);

    context.builder.position_at_end(&false_block);
    let false_value = gen_expr(&if_else.false_br, context);
//    context.builder.build_store(ret_pointer, convert(false_value));
    context.builder.build_unconditional_branch(&end_block);

    context.builder.position_at_end(&end_block);

//    context.builder.build_load(ret_pointer, "load_ret_if_else");
    Value::Void // TODO(tanin): fix this when we have type inference.
}

fn gen_string(
    literal_string: &tree::LiteralString,
    context: &FnContext
) -> Value {
    let i8_type = context.context.i8_type();
    let i32_type = context.context.i32_type();

    let string = context.builder.build_alloca(context.core.string_struct_type, "string");

    let array_type = i8_type.array_type((literal_string.content.len() + 1) as u32);
    let array = context.builder.build_alloca(array_type, "string_array");

    for (index, c) in literal_string.content.chars().enumerate() {
        let p = unsafe {
            context.builder.build_in_bounds_gep(
                array,
                &[i32_type.const_int(0, false), i32_type.const_int(index as u64, false)],
                "gep")
        };
        context.builder.build_store(p, i8_type.const_int(c as u64, false));
    }
    // Store string terminating symbol
    let last = unsafe {
        context.builder.build_in_bounds_gep(
            array,
            &[i32_type.const_int(0, false), i32_type.const_int(literal_string.content.len() as u64, false)],
            "gep")
    };
    context.builder.build_store(last, i8_type.const_int(0, false));

    let size = i32_type.const_int((literal_string.content.len() + 1) as u64, false);

    let size_pointer = unsafe {
        context.builder.build_in_bounds_gep(
            string,
            &[i32_type.const_int(0, false), i32_type.const_int(0, false)],
            "gep"
        )
    };
    context.builder.build_store(size_pointer, size);

    let content_pointer = unsafe {
        context.builder.build_in_bounds_gep(
            string,
            &[i32_type.const_int(0, false), i32_type.const_int(1, false)],
            "gep"
        )
    };
    context.builder.build_store(content_pointer, array);

    Value::String(string)
}

fn gen_string_from_cstring(
    cstring: PointerValue,
    cstring_size: IntValue,
    context: &FnContext
) -> Value {
    let i8_type = context.context.i8_type();
    let i32_type = context.context.i32_type();

    let string = context.builder.build_alloca(context.core.string_struct_type, "string");

    let size_with_terminator = cstring_size.const_add(context.context.i32_type().const_int(1, false));
    let array = context.builder.build_array_alloca(i8_type, size_with_terminator,  "string_array");

    let memcpy = match context.module.get_function("llvm.memcpy.p0i8.p0i8.i64") {
        None => {
           context.module.add_function(
               "llvm.memcpy.p0i8.p0i8.i64",
               context.context.i64_type().fn_type(
                   &[
                       i8_type.ptr_type(AddressSpace::Generic).into(),
                       i8_type.ptr_type(AddressSpace::Generic).into(),
                       context.context.i64_type().into(),
                       context.context.i32_type().into(),
                       context.context.bool_type().into()
                   ],
                   false
               ),
               Some(Linkage::External)
           )
        }
        Some(f) => f,
    };

    context.builder.build_call(
        memcpy,
        &[
            array.into(),
            cstring.into(),
            size_with_terminator.into(),
            context.context.i32_type().const_int(4, false).into(),
            context.context.bool_type().const_zero().into()
        ],
        "memcpy"
    );

    let size_pointer = unsafe {
        context.builder.build_in_bounds_gep(
            string,
            &[i32_type.const_int(0, false), i32_type.const_int(0, false)],
            "gep"
        )
    };
    context.builder.build_store(size_pointer, cstring_size);

    let content_pointer = unsafe {
        context.builder.build_in_bounds_gep(
            string,
            &[i32_type.const_int(0, false), i32_type.const_int(1, false)],
            "gep"
        )
    };
    context.builder.build_store(content_pointer, array);

    Value::String(string)
}

fn gen_read_var(
    var: &tree::ReadVar,
    context: &FnContext,
) -> Value {
    let assignment = unsafe { &*var.assignment_ref.get().unwrap() };
    let i32_type = context.context.i32_type();
    let value = context.builder.build_load(
        assignment.llvm_ref.get().unwrap(),
        &var.name
    );
    match value {
        BasicValueEnum::IntValue(i) => Value::Number(i),
        BasicValueEnum::PointerValue(p) => Value::String(p),
        _ => panic!("Unable to read var")
    }
}

fn gen_assignment(
    assignment: &tree::Assignment,
    context: &FnContext,
) -> Value {
    let expr = gen_expr(&assignment.expr, context);

   let ptr = match expr {
       Value::Number(_) => {
           let i32_type = context.context.i32_type();
           context.builder.build_alloca(i32_type, &assignment.var.name)
       },
       Value::String(p) => {
           let ptr_type = context.core.string_struct_type.ptr_type(AddressSpace::Generic);
           context.builder.build_alloca(ptr_type, &assignment.var.name)
       },
       _ => panic!("Unknow expr")
   } ;


    context.builder.build_store(ptr, convert(expr));
    assignment.var.llvm_ref.replace(Some(ptr));
    Value::Void
}

fn gen_num(
    num: &tree::Num,
    context: &FnContext,
) -> Value {
    let i32_type = context.context.i32_type();
    Value::Number(i32_type.const_int(num.value as u64, false))
}

fn gen_invoke(
    invoke: &tree::Invoke,
    context: &FnContext,
) -> Value {
    if invoke.name == "print" {
        let printf = match context.module.get_function("printf") {
            Some(f) => f,
            None => {
                let str_type = context.context.i8_type().ptr_type(AddressSpace::Generic);
                let i32_type = context.context.i32_type();
                let printf_type = i32_type.fn_type(&[str_type.into()], true);
                context.module.add_function("printf", printf_type, Some(Linkage::External))
            },
        };

        let i32_type = context.context.i32_type();
        let ptr_type = context.context.i32_type().ptr_type(AddressSpace::Generic);
        let arg = gen_expr(&invoke.arg, context);
        let ss = match arg {
            Value::String(ptr) => ptr,
            _ => panic!("fail arg"),
        };
        let s = unsafe {
            context.builder.build_in_bounds_gep(ss, &[i32_type.const_int(0, false), i32_type.const_int(1, false)], "")
        };
        let l = context.builder.build_load(s, "load");
        context.builder.build_call(printf, &[l], "");
        Value::Void
    } else if invoke.name == "read" {
        let io_struct = context.context.opaque_struct_type("struct._IO_FILE");
        let fgets = match context.module.get_function("fgets") {
            Some(f) => f,
            None => {
                let fgets_type = context.context.i8_type().ptr_type(AddressSpace::Generic).fn_type(
                    &[
                        context.context.i8_type().ptr_type(AddressSpace::Generic).into(),
                        context.context.i32_type().into(),
                        io_struct.ptr_type(AddressSpace::Generic).into(),
                    ],
                    false);
                context.module.add_function("fgets", fgets_type, Some(Linkage::External))
            },
        };
        let stdin = match context.module.get_global("stdin") {
            None => {
                let g = context.module.add_global(
                    io_struct.ptr_type(AddressSpace::Generic),
                    None,
                    "stdin");
                g
            },
            Some(g) => g
        };

        let input_size = 100;
        let input = context.builder.build_alloca(context.context.i8_type().array_type(input_size), "input");
        context.builder.build_call(
            fgets,
            &[
                input.into(),
                context.context.i32_type().const_int(input_size as u64, false).into(),
                context.builder.build_load(stdin.as_pointer_value(), "load_stdin"),
            ],
            "fgets").try_as_basic_value().left().unwrap();
        let strlen = match context.module.get_function("strlen") {
            Some(f) => f,
            None => {
                let fn_type = context.context.i64_type().fn_type(
                    &[
                        context.context.i8_type().ptr_type(AddressSpace::Generic).into()
                    ],
                    false);
                context.module.add_function("strlen", fn_type, Some(Linkage::External))
            },
        };
        let ret_str_len = context.builder.build_call(strlen, &[input.into()], "strlen");
        let size = match ret_str_len.try_as_basic_value().left().unwrap() {
            BasicValueEnum::IntValue(i) => i,
            _ => panic!("unable to get string's length")
        };
        gen_string_from_cstring(input, size, context)
    } else if invoke.name == "parseNumber" {
        let parse_number = match context.module.get_function("strtol") {
            Some(f) => f,
            None => {
                let str_type = context.context.i8_type().ptr_type(AddressSpace::Generic);
                let str_end_type = context.context.i8_type().ptr_type(AddressSpace::Generic).ptr_type(AddressSpace::Generic);
                let base_type = context.context.i32_type();
                let fn_type = context.context.i32_type().fn_type(&[str_type.into(), str_end_type.into(), base_type.into()], false);
                context.module.add_function("strtol", fn_type, Some(Linkage::External))
            },
        };

        let i32_type = context.context.i32_type();
        let ptr_type = context.context.i32_type().ptr_type(AddressSpace::Generic);
        let arg = gen_expr(&invoke.arg, context);
        let ss = match arg {
            Value::String(ptr) => ptr,
            _ => panic!("fail arg"),
        };
        let s = unsafe {
            context.builder.build_in_bounds_gep(ss, &[i32_type.const_int(0, false), i32_type.const_int(1, false)], "")
        };
        let l = context.builder.build_load(s, "load");
        let ret = context.builder.build_call(parse_number, &[l.into(), context.context.i8_type().ptr_type(AddressSpace::Generic).ptr_type(AddressSpace::Generic).const_null().into(), context.context.i32_type().const_int(10, false).into()], "").try_as_basic_value().left().unwrap();
        match ret {
            BasicValueEnum::IntValue(i) => Value::Number(i),
            _ => panic!("fail")
        }
    } else {
        let func = unsafe { &*invoke.func_ref.get().unwrap() };
        context.builder.build_call(func.llvm_ref.get().unwrap(), &[], &invoke.name).try_as_basic_value().left().unwrap();
        Value::Void
    }
}
