//! This file will run at build time to autogenerate Rust tests based on
//! WebAssembly spec tests. It will convert the files indicated in TESTS
//! from "/spectests/{MODULE}.wast" to "/src/spectests/{MODULE}.rs".
use std::collections::HashMap;
use std::fs::File;
use std::path::PathBuf;
use std::{env, fs, io::Write};
use wabt::script::{Action, Command, CommandKind, ModuleBinary, ScriptParser, Value};
use wabt::wasm2wat;

static BANNER: &str = "// Rust test file autogenerated with cargo build (build/spectests.rs).
// Please do NOT modify it by hand, as it will be reset on next build.\n";

const TESTS: &[&str] = &[
    "spectests/address.wast",
    "spectests/align.wast",
    "spectests/binary.wast",
    "spectests/block.wast",
    "spectests/br.wast",
    "spectests/br_if.wast",
    "spectests/br_table.wast",
    "spectests/break_drop.wast",
    "spectests/call.wast",
    "spectests/call_indirect.wast",
    "spectests/comments.wast",
    "spectests/const_.wast",
    "spectests/conversions.wast",
    "spectests/custom.wast",
    "spectests/data.wast",
    "spectests/elem.wast",
    "spectests/endianness.wast",
    "spectests/exports.wast",
    "spectests/f32_.wast",
    "spectests/f32_bitwise.wast",
    "spectests/f32_cmp.wast",
    "spectests/f64_.wast",
    "spectests/f64_bitwise.wast",
    "spectests/f64_cmp.wast",
    "spectests/fac.wast",
    "spectests/float_exprs.wast",
    "spectests/float_literals.wast",
    "spectests/float_memory.wast",
    "spectests/float_misc.wast",
    "spectests/forward.wast",
    "spectests/func.wast",
    "spectests/func_ptrs.wast",
    "spectests/get_local.wast",
    "spectests/globals.wast",
    "spectests/i32_.wast",
    "spectests/i64_.wast",
    "spectests/if_.wast",
    "spectests/int_exprs.wast",
    "spectests/int_literals.wast",
    "spectests/labels.wast",
    "spectests/left_to_right.wast",
    "spectests/loop_.wast",
    "spectests/memory.wast",
    "spectests/memory_grow.wast",
    "spectests/memory_redundancy.wast",
    "spectests/memory_trap.wast",
    "spectests/nop.wast",
    "spectests/return_.wast",
    "spectests/select.wast",
    "spectests/set_local.wast",
    "spectests/stack.wast",
    "spectests/start.wast",
    "spectests/store_retval.wast",
    "spectests/switch.wast",
    "spectests/tee_local.wast",
    "spectests/token.wast",
    "spectests/traps.wast",
    "spectests/typecheck.wast",
    "spectests/types.wast",
    "spectests/unwind.wast",
];

static COMMON: &'static str = r##"
use std::{{f32, f64}};
use wabt::wat2wasm;
use wasmer_clif_backend::CraneliftCompiler;
use wasmer_runtime_core::import::ImportObject;
use wasmer_runtime_core::types::Value;
use wasmer_runtime_core::{{Instance, module::Module}};
use wasmer_runtime_core::error::Result;

static IMPORT_MODULE: &str = r#"
(module
  (type $t0 (func (param i32)))
  (type $t1 (func))
  (func $print_i32 (export "print_i32") (type $t0) (param $lhs i32))
  (func $print (export "print") (type $t1))
  (table $table (export "table") 10 20 anyfunc)
  (memory $memory (export "memory") 1 2)
  (global $global_i32 (export "global_i32") i32 (i32.const 666)))
"#;

pub fn generate_imports() -> ImportObject {
    let wasm_binary = wat2wasm(IMPORT_MODULE.as_bytes()).expect("WAST not valid or malformed");
    let module = wasmer_runtime_core::compile_with(&wasm_binary[..], &CraneliftCompiler::new())
        .expect("WASM can't be compiled");
    let instance = module
        .instantiate(&ImportObject::new())
        .expect("WASM can't be instantiated");
    let mut imports = ImportObject::new();
    imports.register("spectest", instance);
    imports
}

/// Bit pattern of an f32 value:
///     1-bit sign + 8-bit mantissa + 23-bit exponent = 32 bits
///
/// Bit pattern of an f64 value:
///     1-bit sign + 11-bit mantissa + 52-bit exponent = 64 bits
///
/// NOTE: On some old platforms (PA-RISC, some MIPS) quiet NaNs (qNaN) have
/// their mantissa MSB unset and set for signaling NaNs (sNaN).
///
/// Links:
///     * https://en.wikipedia.org/wiki/Floating-point_arithmetic
///     * https://github.com/WebAssembly/spec/issues/286
///     * https://en.wikipedia.org/wiki/NaN
///
pub trait NaNCheck {
    fn is_quiet_nan(&self) -> bool;
    fn is_canonical_nan(&self) -> bool;
}

impl NaNCheck for f32 {
    /// The MSB of the mantissa must be set for a NaN to be a quiet NaN.
    fn is_quiet_nan(&self) -> bool {
        let bit_mask = 0b1 << 22; // Used to check if 23rd bit is set, which is MSB of the mantissa
        self.is_nan() && (self.to_bits() & bit_mask) == bit_mask
    }

    /// For a NaN to be canonical, its mantissa bits must all be unset
    fn is_canonical_nan(&self) -> bool {
        let bit_mask: u32 = 0b1____0000_0000____011_1111_1111_1111_1111_1111;
        let masked_value = self.to_bits() ^ bit_mask;
        masked_value == 0xFFFF_FFFF || masked_value == 0x7FFF_FFFF
    }
}

impl NaNCheck for f64 {
    /// The MSB of the mantissa must be set for a NaN to be a quiet NaN.
    fn is_quiet_nan(&self) -> bool {
        let bit_mask = 0b1 << 51; // Used to check if 52st bit is set, which is MSB of the mantissa
        self.is_nan() && (self.to_bits() & bit_mask) == bit_mask
    }

    /// For a NaN to be canonical, its mantissa bits must all be unset
    fn is_canonical_nan(&self) -> bool {
        let bit_mask: u64 =
            0b1____000_0000_0000____0111_1111_1111_1111_1111_1111_1111_1111_1111_1111_1111_1111_1111;
        let masked_value = self.to_bits() ^ bit_mask;
        masked_value == 0x7FFF_FFFF_FFFF_FFFF || masked_value == 0xFFF_FFFF_FFFF_FFFF
    }
}
"##;

fn wabt2rust_type(v: &Value) -> String {
    match v {
        Value::I32(_v) => format!("i32"),
        Value::I64(_v) => format!("i64"),
        Value::F32(_v) => format!("f32"),
        Value::F64(_v) => format!("f64"),
    }
}

fn wabt2rust_type_destructure(v: &Value, placeholder: &str) -> String {
    match v {
        Value::I32(_v) => format!("Value::I32({})", placeholder),
        Value::I64(_v) => format!("Value::I64({})", placeholder),
        Value::F32(_v) => format!("Value::F32({})", placeholder),
        Value::F64(_v) => format!("Value::F64({})", placeholder),
    }
}

fn is_nan(v: &Value) -> bool {
    if let Value::F32(v) = v {
        return v.is_nan();
    } else if let Value::F64(v) = v {
        return v.is_nan();
    }
    return false;
}

fn wabt2rust_value_bare(v: &Value) -> String {
    match v {
        Value::I32(v) => format!("{:?} as i32", v),
        Value::I64(v) => format!("{:?} as i64", v),
        Value::F32(v) => {
            if v.is_infinite() {
                if v.is_sign_negative() {
                    "f32::NEG_INFINITY".to_string()
                } else {
                    "f32::INFINITY".to_string()
                }
            } else if v.is_nan() {
                // Support for non-canonical NaNs
                format!("f32::from_bits({:?})", v.to_bits())
            } else {
                format!("{:?}", v)
            }
        }
        Value::F64(v) => {
            if v.is_infinite() {
                if v.is_sign_negative() {
                    "f64::NEG_INFINITY".to_string()
                } else {
                    "f64::INFINITY".to_string()
                }
            } else if v.is_nan() {
                format!("f64::from_bits({:?})", v.to_bits())
            } else {
                format!("{:?}", v)
            }
        }
    }
}

fn wabt2rust_value(v: &Value) -> String {
    match v {
        Value::I32(v) => format!("Value::I32({:?} as i32)", v),
        Value::I64(v) => format!("Value::I64({:?} as i64)", v),
        Value::F32(v) => {
            if v.is_infinite() {
                if v.is_sign_negative() {
                    "Value::F32(f32::NEG_INFINITY)".to_string()
                } else {
                    "Value::F32(f32::INFINITY)".to_string()
                }
            } else if v.is_nan() {
                // Support for non-canonical NaNs
                format!("Value::F32(f32::from_bits({:?}))", v.to_bits())
            } else {
                format!("Value::F32(({:?}f32))", v)
            }
        }
        Value::F64(v) => {
            if v.is_infinite() {
                if v.is_sign_negative() {
                    "Value::F64(f64::NEG_INFINITY)".to_string()
                } else {
                    "Value::F64(f64::INFINITY)".to_string()
                }
            } else if v.is_nan() {
                format!("Value::F64(f64::from_bits({:?}))", v.to_bits())
            } else {
                format!("Value::F64(({:?}f64))", v)
            }
        }
    }
}

struct WastTestGenerator {
    last_module: i32,
    last_line: u64,
    command_no: i32,
    script_parser: ScriptParser,
    module_calls: HashMap<i32, Vec<String>>,
    buffer: String,
}

impl WastTestGenerator {
    fn new(path: &PathBuf) -> Self {
        let filename = path.file_name().unwrap().to_str().unwrap();
        let source = fs::read(&path).unwrap();
        let script: ScriptParser = ScriptParser::from_source_and_name(&source, filename).unwrap();
        let buffer = String::new();
        WastTestGenerator {
            last_module: 0,
            last_line: 0,
            command_no: 0,
            script_parser: script,
            buffer: buffer,
            module_calls: HashMap::new(),
        }
    }

    fn is_fat_test(&self) -> bool {
        self.command_no > 200
    }

    fn consume(&mut self) {
        self.buffer.push_str(BANNER);
        //         self.buffer.push_str(&format!(
        //             "// Test based on spectests/{}
        // #![allow(
        //     warnings,
        //     dead_code
        // )]
        // //use wabt::wat2wasm;
        // use std::{{f32, f64}};

        // use wasmer_runtime_core::types::Value;
        // use wasmer_runtime_core::{{Instance, module::Module}};

        // //use crate::spectests::_common::{{
        // //    generate_imports,
        // //    NaNCheck,
        // //}};\n\n",
        //             self.filename
        //         ));
        while let Some(Command { line, kind }) = &self.script_parser.next().unwrap() {
            self.last_line = line.clone();
            self.buffer
                .push_str(&format!("\n// Line {}\n", self.last_line));
            self.visit_command(&kind);
            self.command_no = self.command_no + 1;
        }
        for n in 1..self.last_module + 1 {
            self.flush_module_calls(n);
        }
    }

    fn command_name(&self) -> String {
        format!("c{}_l{}", self.command_no, self.last_line)
    }

    fn flush_module_calls(&mut self, module: i32) {
        let calls: Vec<String> = self
            .module_calls
            .entry(module)
            .or_insert(Vec::new())
            .iter()
            .map(|call_str| format!("{}(&mut instance);", call_str))
            .collect();
        if calls.len() > 0 {
            self.buffer.push_str(
                format!(
                    "\n#[test]
fn test_module_{}() {{
    let mut instance = create_module_{}();
    // We group the calls together
    {}
}}\n",
                    module,
                    module,
                    calls.join("\n    ")
                )
                .as_str(),
            );
        }
        self.module_calls.remove(&module);
    }

    fn visit_module(&mut self, module: &ModuleBinary, _name: &Option<String>) {
        let wasm_binary: Vec<u8> = module.clone().into_vec();
        let wast_string = wasm2wat(wasm_binary).expect("Can't convert back to wasm");
        let last_module = self.last_module;
        self.flush_module_calls(last_module);
        self.last_module = self.last_module + 1;
        // self.module_calls.insert(self.last_module, vec![]);
        self.buffer.push_str(
            format!(
                "fn create_module_{}() -> Instance {{
    let module_str = \"{}\";
    println!(\"{{}}\", module_str);
    let wasm_binary = wat2wasm(module_str.as_bytes()).expect(\"WAST not valid or malformed\");
    let module = wasmer_runtime_core::compile_with(&wasm_binary[..], &CraneliftCompiler::new()).expect(\"WASM can't be compiled\");
    module.instantiate(&generate_imports()).expect(\"WASM can't be instantiated\")
}}\n",
                self.last_module,
                // We do this to ident four spaces, so it looks aligned to the function body
                wast_string
                    .replace("\n", "\n    ")
                    .replace("\\", "\\\\")
                    .replace("\"", "\\\""),
            )
            .as_str(),
        );

        // We set the start call to the module
        let start_module_call = format!("start_module_{}", self.last_module);
        self.buffer.push_str(
            format!(
                "\nfn {}(vmctx: &mut Ctx) {{
    // TODO Review is explicit start needed? Start now called in runtime::Instance::new()
    //instance.start();
}}\n",
                start_module_call
            )
            .as_str(),
        );
        self.module_calls
            .entry(self.last_module)
            .or_insert(Vec::new())
            .push(start_module_call);
    }

    fn visit_assert_invalid(&mut self, module: &ModuleBinary) {
        let wasm_binary: Vec<u8> = module.clone().into_vec();
        // let wast_string = wasm2wat(wasm_binary).expect("Can't convert back to wasm");
        let command_name = self.command_name();
        self.buffer.push_str(
            format!(
                "#[test]
fn {}_assert_invalid() {{
    let wasm_binary = {:?};
    let module = wasmer_runtime_core::compile_with(&wasm_binary, &CraneliftCompiler::new());
    assert!(module.is_err(), \"WASM should not compile as is invalid\");
}}\n",
                command_name,
                wasm_binary,
                // We do this to ident four spaces back
                // String::from_utf8_lossy(&wasm_binary),
                // wast_string.replace("\n", "\n    "),
            )
            .as_str(),
        );
    }

    // TODO: Refactor repetitive code
    fn visit_assert_return_arithmetic_nan(&mut self, action: &Action) {
        match action {
            Action::Invoke {
                module: _,
                field,
                args,
            } => {
                // let return_type = wabt2rust_type(&args[0]);
                // let func_return = format!(" -> {}", return_type);
                let assertion = String::from(
                    "assert!(match result {
        Value::F32(fp) => fp.is_quiet_nan(),
        Value::F64(fp) => fp.is_quiet_nan(),
        _ => unimplemented!()
    })",
                );

                // We map the arguments provided into the raw Arguments provided
                // to libffi
                // let args_types: Vec<String> = args.iter().map(wabt2rust_type).collect();
                //                args_types.push("&Instance".to_string());
                let args_values: Vec<String> = args.iter().map(wabt2rust_value).collect();
                //                args_values.push("&result_object.instance".to_string());
                let func_name = format!("{}_assert_return_arithmetic_nan", self.command_name());
                self.buffer.push_str(
                    format!(
                        "fn {func_name}(vmctx: &mut Ctx) {{
    println!(\"Executing function {{}}\", \"{func_name}\");
    let result = instance.call(\"{field}\", &[{args_values}]).unwrap().first().expect(\"Missing result in {func_name}\").clone();
    {assertion}
}}\n",
                        func_name=func_name,
                        field=field,
                        args_values=args_values.join(", "),
                        assertion=assertion,
                    )
                    .as_str(),
                );
                //                field=field,
                //                args_types=args_types.join(", "),
                //                func_return=func_return,
                self.module_calls
                    .entry(self.last_module)
                    .or_insert(Vec::new())
                    .push(func_name);
                // let mut module_calls = self.module_calls.get(&self.last_module).unwrap();
                // module_calls.push(func_name);
            }
            _ => {}
        };
    }

    // PROBLEM: Im assuming the return type from the first argument type
    // and wabt does gives us the `expected` result
    // TODO: Refactor repetitive code
    fn visit_assert_return_canonical_nan(&mut self, action: &Action) {
        match action {
            Action::Invoke {
                module: _,
                field,
                args,
            } => {
                let _return_type = match &field.as_str() {
                    &"f64.promote_f32" => String::from("f64"),
                    &"f32.promote_f64" => String::from("f32"),
                    _ => wabt2rust_type(&args[0]),
                };
                // let func_return = format!(" -> {}", return_type);
                let assertion = String::from(
                    "assert!(match result {
        Value::F32(fp) => fp.is_quiet_nan(),
        Value::F64(fp) => fp.is_quiet_nan(),
        _ => unimplemented!()
    })",
                );

                // We map the arguments provided into the raw Arguments provided
                // to libffi
                // let args_types: Vec<String> = args.iter().map(wabt2rust_type).collect();
                //                args_types.push("&Instance".to_string());
                let args_values: Vec<String> = args.iter().map(wabt2rust_value).collect();
                //                args_values.push("&result_object.instance".to_string());
                let func_name = format!("{}_assert_return_canonical_nan", self.command_name());
                self.buffer.push_str(
                    format!(
                        "fn {func_name}(vmctx: &mut Ctx) {{
    println!(\"Executing function {{}}\", \"{func_name}\");
    let result = instance.call(\"{field}\", &[{args_values}]).unwrap().first().expect(\"Missing result in {func_name}\").clone();
    {assertion}
}}\n",
                        func_name=func_name,
                        field=field,
                        args_values=args_values.join(", "),
                        assertion=assertion,
                    )
                    .as_str(),
                );
                self.module_calls
                    .entry(self.last_module)
                    .or_insert(Vec::new())
                    .push(func_name);
                // let mut module_calls = self.module_calls.get(&self.last_module).unwrap();
                // module_calls.push(func_name);
            }
            _ => {}
        };
    }

    fn visit_assert_malformed(&mut self, module: &ModuleBinary) {
        let wasm_binary: Vec<u8> = module.clone().into_vec();
        let command_name = self.command_name();
        // let wast_string = wasm2wat(wasm_binary).expect("Can't convert back to wasm");
        self.buffer.push_str(
            format!(
                "#[test]
fn {}_assert_malformed() {{
    let wasm_binary = {:?};
    let compilation = wasmer_runtime_core::compile_with(&wasm_binary, &CraneliftCompiler::new());
    assert!(compilation.is_err(), \"WASM should not compile as is malformed\");
}}\n",
                command_name,
                wasm_binary,
                // We do this to ident four spaces back
                // String::from_utf8_lossy(&wasm_binary),
                // wast_string.replace("\n", "\n    "),
            )
            .as_str(),
        );
    }

    // TODO: Refactor repetitive code
    fn visit_action(&mut self, action: &Action, expected: Option<&Vec<Value>>) -> Option<String> {
        match action {
            Action::Invoke {
                module: _,
                field,
                args,
            } => {
                let (_func_return, assertion) = match expected {
                    Some(expected) => {
                        let func_return = if expected.len() > 0 {
                            format!(" -> {}", wabt2rust_type(&expected[0]))
                        } else {
                            "".to_string()
                        };
                        let expected_result = if expected.len() > 0 {
                            wabt2rust_value_bare(&expected[0])
                        } else {
                            "should not use this expect result".to_string()
                        };
                        let expected_vec_result = if expected.len() > 0 {
                            format!("Ok(vec![{}])", wabt2rust_value(&expected[0]))
                        } else {
                            "Ok(vec![])".to_string()
                        };
                        let return_type = if expected.len() > 0 {
                            wabt2rust_type(&expected[0])
                        } else {
                            "should not use this return type".to_string()
                        };
                        let return_type_destructure = if expected.len() > 0 {
                            wabt2rust_type_destructure(&expected[0], "result")
                        } else {
                            "should not use this result return type destructure".to_string()
                        };
                        let _expected_type_destructure = if expected.len() > 0 {
                            wabt2rust_type_destructure(&expected[0], "expected")
                        } else {
                            "should not use this expected return type destructure".to_string()
                        };
                        let assertion = if expected.len() > 0 && is_nan(&expected[0]) {
                            format!(
                                "let expected = {expected_result};
                                if let {return_type_destructure} = result.clone().unwrap().first().unwrap() {{
                                assert!((*result as {return_type}).is_nan());
            assert_eq!((*result as {return_type}).is_sign_positive(), (expected as {return_type}).is_sign_positive());
            }} else {{
              panic!(\"Unexpected result type {{:?}}\", result);
            }}",
                                expected_result=expected_result,
                                return_type=return_type,
                                return_type_destructure=return_type_destructure
                            )
                        } else {
                            format!("assert_eq!(result, {});", expected_vec_result)
                        };
                        (func_return, assertion)
                    }
                    None => ("".to_string(), "".to_string()),
                };

                // We map the arguments provided into the raw Arguments provided
                // to libffi
                // let mut args_types: Vec<String> = args.iter().map(wabt2rust_type).collect();
                //                args_types.push("&Instance".to_string());
                let args_values: Vec<String> = args.iter().map(wabt2rust_value).collect();
                //                args_values.push("&result_object.instance".to_string());
                let func_name = format!("{}_action_invoke", self.command_name());
                self.buffer.push_str(
                    format!(
                        "fn {func_name}(vmctx: &mut Ctx) -> Result<()> {{
    println!(\"Executing function {{}}\", \"{func_name}\");
    let result = instance.call(\"{field}\", &[{args_values}]);
    {assertion}
    result?;
    Ok(())
}}\n",
                        func_name = func_name,
                        field = field,
                        args_values = args_values.join(", "),
                        assertion = assertion,
                    )
                    .as_str(),
                );
                Some(func_name)
                // let mut module_calls = self.module_calls.get(&self.last_module).unwrap();
                // module_calls.push(func_name);
            }
            _ => None,
        }
    }

    fn visit_assert_return(&mut self, action: &Action, expected: &Vec<Value>) {
        let action_fn_name = self.visit_action(action, Some(expected));

        if action_fn_name.is_none() {
            return;
        }
        self.module_calls
            .entry(self.last_module)
            .or_insert(Vec::new())
            .push(action_fn_name.unwrap());
    }

    fn visit_perform_action(&mut self, action: &Action) {
        let action_fn_name = self.visit_action(action, None);

        if action_fn_name.is_none() {
            return;
        }
        self.module_calls
            .entry(self.last_module)
            .or_insert(Vec::new())
            .push(action_fn_name.unwrap());
    }

    fn visit_assert_trap(&mut self, action: &Action) {
        let action_fn_name = self.visit_action(action, None);

        if action_fn_name.is_none() {
            return;
        }
        let trap_func_name = format!("{}_assert_trap", self.command_name());
        self.buffer.push_str(
            format!(
                "
#[test]
fn {}() {{
    let mut instance = create_module_{}();
    let result = {}(&mut instance);
    assert!(result.is_err());
}}\n",
                trap_func_name,
                self.last_module,
                action_fn_name.unwrap(),
            )
            .as_str(),
        );

        // We don't group trap calls as they may cause memory faults
        // on the instance memory. So we test them alone.
        // self.module_calls
        //     .entry(self.last_module)
        //     .or_insert(Vec::new())
        //     .push(trap_func_name);
    }

    fn visit_command(&mut self, cmd: &CommandKind) {
        match cmd {
            CommandKind::Module { module, name } => {
                self.visit_module(module, name);
            }
            CommandKind::AssertReturn { action, expected } => {
                self.visit_assert_return(action, expected)
            }
            CommandKind::AssertReturnCanonicalNan { action } => {
                self.visit_assert_return_canonical_nan(action);
            }
            CommandKind::AssertReturnArithmeticNan { action } => {
                self.visit_assert_return_arithmetic_nan(action);
            }
            CommandKind::AssertTrap { action, message: _ } => {
                self.visit_assert_trap(action);
            }
            CommandKind::AssertInvalid { module, message: _ } => {
                self.visit_assert_invalid(module);
            }
            CommandKind::AssertMalformed { module, message: _ } => {
                self.visit_assert_malformed(module);
            }
            CommandKind::AssertUninstantiable {
                module: _,
                message: _,
            } => {
                // Do nothing for now
            }
            CommandKind::AssertExhaustion { action: _ } => {
                // Do nothing for now
            }
            CommandKind::AssertUnlinkable {
                module: _,
                message: _,
            } => {
                // Do nothing for now
            }
            CommandKind::Register {
                name: _,
                as_name: _,
            } => {
                // Do nothing for now
            }
            CommandKind::PerformAction(action) => {
                self.visit_perform_action(action);
            }
        }
    }
    fn finalize(&self) -> &String {
        &self.buffer
    }
}

fn generate_spectest(out: &mut File, test_name: &str, wast: &PathBuf) -> std::io::Result<()> {
    let mut generator = WastTestGenerator::new(wast);
    generator.consume();
    let generated_script = generator.finalize();

    if !generator.is_fat_test() {
        out.write(format!("mod test_{} {{\nuse super::*;\n", test_name).as_bytes())?;
        out.write(generated_script.as_bytes())?;
        out.write("\n}\n".as_bytes())?;
    }

    Ok(())
}

pub fn build() -> std::io::Result<()> {
    let mut out_file = File::create(format!("{}/spectests.rs", env::var("OUT_DIR").unwrap()))?;

    out_file.write(COMMON.as_bytes())?;

    for test in TESTS.iter() {
        let mut wast_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        wast_path.push(test);
        generate_spectest(
            &mut out_file,
            test.split("/").last().unwrap().split(".").next().unwrap(),
            &wast_path,
        )?
    }

    Ok(())
}
