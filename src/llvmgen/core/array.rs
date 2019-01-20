use inkwell::values::BasicValueEnum;
use llvmgen::gen::FnContext;
use llvmgen::gen::Value;

use llvmgen::native;
use llvmgen::core;
use inkwell::values::IntValue;

pub fn instantiate_from_value(value: BasicValueEnum, size: Value, context: &FnContext) -> Value {
    let native_array_ptr = match native::array::instantiate_from_value(value, context) {
        Value::Class(p, c) => p,
        x => panic!("Expect Value::Class, found {:?}", x),
    };
    let size_ptr = match size {
        Value::Class(p, c) => p,
        x => panic!("Expect Value::Class, found {:?}", x),
    };

    let instance_ptr = native::gen_malloc(&context.core.array_class.llvm_struct_type_ref.get().unwrap(), context);
    let first_param_pointer = unsafe { context.builder.build_struct_gep(instance_ptr, 0, "first_param_of_array") };
    context.builder.build_store(first_param_pointer, native_array_ptr);
    let second_param_pointer = unsafe { context.builder.build_struct_gep(instance_ptr, 1, "second_param_of_array") };
    context.builder.build_store(second_param_pointer, size_ptr);
    Value::Class(instance_ptr, context.core.array_class)
}
