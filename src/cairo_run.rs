use crate::prelude::*;

use crate::{
    hint_processor::hint_processor_definition::HintProcessor,
    types::program::Program,
    vm::{
        errors::{cairo_run_errors::CairoRunError, vm_exception::VmException},
        runners::cairo_runner::CairoRunner,
        security::verify_secure_runner,
        vm_core::VirtualMachine,
    },
};

#[cfg(feature = "std")]
use felt::Felt;

#[cfg(feature = "std")]
use std::{
    fs::File,
    io::{self, BufWriter, Write},
    path::Path,
};

pub struct CairoRunConfig<'a> {
    pub entrypoint: &'a str,
    pub trace_enabled: bool,
    pub layout: &'a str,
    pub proof_mode: bool,
    pub secure_run: Option<bool>,
}

impl<'a> Default for CairoRunConfig<'a> {
    fn default() -> Self {
        CairoRunConfig {
            entrypoint: "main",
            trace_enabled: false,
            layout: "plain",
            proof_mode: false,
            secure_run: None,
        }
    }
}

pub fn cairo_run(
    program_content: &[u8],
    cairo_run_config: &CairoRunConfig,
    hint_executor: &mut dyn HintProcessor,
    output_dest: Option<&mut String>,
) -> Result<CairoRunner, CairoRunError> {
    let program = Program::from_bytes(program_content, Some(cairo_run_config.entrypoint))?;

    let secure_run = cairo_run_config
        .secure_run
        .unwrap_or(!cairo_run_config.proof_mode);

    let mut cairo_runner = CairoRunner::new(
        &program,
        cairo_run_config.layout,
        cairo_run_config.proof_mode,
    )?;
    let mut vm = VirtualMachine::new(cairo_run_config.trace_enabled);
    let end = cairo_runner.initialize(&mut vm)?;

    cairo_runner
        .run_until_pc(end, &mut vm, hint_executor)
        .map_err(|err| VmException::from_vm_error(&cairo_runner, &vm, err))?;
    cairo_runner.end_run(false, false, &mut vm, hint_executor)?;

    vm.verify_auto_deductions()?;
    cairo_runner.read_return_values(&mut vm)?;
    if cairo_run_config.proof_mode {
        cairo_runner.finalize_segments(&mut vm)?;
    }
    if secure_run {
        verify_secure_runner(&cairo_runner, true, &mut vm)?;
    }
    cairo_runner.relocate(&mut vm)?;

    if let Some(output_dest) = output_dest {
        write_output(&mut cairo_runner, &mut vm, output_dest)?;
    }

    Ok(cairo_runner)
}

pub fn write_output(
    cairo_runner: &mut CairoRunner,
    vm: &mut VirtualMachine,
    buffer: &mut String,
) -> Result<(), CairoRunError> {
    cairo_runner.write_output(vm, buffer)?;
    Ok(())
}

/// Writes a trace as a binary file. Bincode encodes to little endian by default and each trace
/// entry is composed of 3 usize values that are padded to always reach 64 bit size.
#[cfg(feature = "std")]
pub fn write_binary_trace(
    relocated_trace: &[crate::vm::trace::trace_entry::RelocatedTraceEntry],
    trace_file: &Path,
) -> io::Result<()> {
    let file = File::create(trace_file)?;
    let mut buffer = BufWriter::new(file);

    for (i, entry) in relocated_trace.iter().enumerate() {
        bincode::serde::encode_into_std_write(entry, &mut buffer, bincode::config::legacy())
            .map_err(|e| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("Failed to dump trace at position {i}, serialize error: {e}"),
                )
            })?;
    }

    buffer.flush()
}

/*
   Writes a binary memory file with the relocated memory as input.
   The memory pairs (address, value) are encoded and concatenated in the file
   given by the path `memory_file`.

   * address -> 8-byte encoded
   * value -> 32-byte encoded
*/
#[cfg(feature = "std")]
pub fn write_binary_memory(
    relocated_memory: &[Option<Felt>],
    memory_file: &Path,
) -> io::Result<()> {
    let file = File::create(memory_file)?;
    let mut buffer = BufWriter::new(file);

    // initialize bytes vector that will be dumped to file
    let mut memory_bytes: Vec<u8> = Vec::new();

    for (i, memory_cell) in relocated_memory.iter().enumerate() {
        match memory_cell {
            None => continue,
            Some(unwrapped_memory_cell) => {
                encode_relocated_memory(&mut memory_bytes, i, unwrapped_memory_cell);
            }
        }
    }

    buffer.write_all(&memory_bytes)?;
    buffer.flush()
}

// encodes a given memory cell.
#[cfg(feature = "std")]
fn encode_relocated_memory(memory_bytes: &mut Vec<u8>, addr: usize, memory_cell: &Felt) {
    // append memory address to bytes vector using a 8 bytes representation
    let mut addr_bytes = (addr as u64).to_le_bytes().to_vec();
    memory_bytes.append(&mut addr_bytes);

    // append memory value at address using a 32 bytes representation
    let mut value_bytes = memory_cell.to_signed_bytes_le();
    value_bytes.resize(32, 0);
    memory_bytes.append(&mut value_bytes);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        hint_processor::{
            builtin_hint_processor::builtin_hint_processor_definition::BuiltinHintProcessor,
            hint_processor_definition::HintProcessor,
        },
        utils::test_utils::*,
    };
    use felt::Felt;

    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::*;

    fn run_test_program(
        program_content: &[u8],
        hint_processor: &mut dyn HintProcessor,
    ) -> Result<(CairoRunner, VirtualMachine), CairoRunError> {
        let program = Program::from_bytes(program_content, Some("main")).unwrap();
        let mut cairo_runner = cairo_runner!(program);
        let mut vm = vm!(true);
        let end = cairo_runner
            .initialize(&mut vm)
            .map_err(CairoRunError::Runner)?;

        assert!(cairo_runner
            .run_until_pc(end, &mut vm, hint_processor)
            .is_ok());

        Ok((cairo_runner, vm))
    }

    #[test]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
    fn cairo_run_custom_entry_point() {
        let program = load_program!("../cairo_programs/not_main.json", Some("not_main"));
        let mut vm = vm!();
        let mut hint_processor = BuiltinHintProcessor::new_empty();
        let mut cairo_runner = cairo_runner!(program);

        let end = cairo_runner.initialize(&mut vm).unwrap();
        assert!(cairo_runner
            .run_until_pc(end, &mut vm, &mut hint_processor)
            .is_ok());
        assert!(cairo_runner.relocate(&mut vm).is_ok());
        // `main` returns without doing nothing, but `not_main` sets `[ap]` to `1`
        // Memory location was found empirically and simply hardcoded
        assert_eq!(cairo_runner.relocated_memory[2], Some(Felt::new(123)));
    }

    #[cfg(feature = "std")]
    fn compare_files(file_path_1: &Path, file_path_2: &Path) -> io::Result<()> {
        use std::io::Read;

        let mut file_1 = File::open(file_path_1)?;
        let mut file_2 = File::open(file_path_2)?;

        let mut buffer_1 = Vec::new();
        let mut buffer_2 = Vec::new();

        file_1.read_to_end(&mut buffer_1)?;
        file_2.read_to_end(&mut buffer_2)?;

        assert_eq!(&buffer_1.len(), &buffer_2.len());

        for (buf_byte_1, buf_byte_2) in buffer_1.iter().zip(buffer_2.iter()) {
            assert_eq!(buf_byte_1, buf_byte_2);
        }
        Ok(())
    }

    #[test]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
    fn cairo_run_with_no_data_program() {
        // a compiled program with no `data` key.
        // it should fail when the program is loaded.
        let mut hint_processor = BuiltinHintProcessor::new_empty();
        let no_data_program_path =
            include_str!("../cairo_programs/manually_compiled/no_data_program.json").as_bytes();
        let cairo_run_config = CairoRunConfig::default();
        assert!(cairo_run(
            no_data_program_path,
            &cairo_run_config,
            &mut hint_processor,
            None
        )
        .is_err());
    }

    #[test]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
    fn cairo_run_with_no_main_program() {
        // a compiled program with no main scope
        // it should fail when trying to run initialize_main_entrypoint.
        let mut hint_processor = BuiltinHintProcessor::new_empty();
        let no_main_program =
            include_str!("../cairo_programs/manually_compiled/no_main_program.json").as_bytes();
        let cairo_run_config = CairoRunConfig::default();
        assert!(cairo_run(
            no_main_program,
            &cairo_run_config,
            &mut hint_processor,
            None
        )
        .is_err());
    }

    #[test]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
    fn cairo_run_with_invalid_memory() {
        // the program invalid_memory.json has an invalid memory cell and errors when trying to
        // decode the instruction.
        let mut hint_processor = BuiltinHintProcessor::new_empty();
        let invalid_memory =
            include_str!("../cairo_programs/manually_compiled/invalid_memory.json").as_bytes();
        let cairo_run_config = CairoRunConfig::default();
        assert!(cairo_run(invalid_memory, &cairo_run_config, &mut hint_processor, None).is_err());
    }

    #[test]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
    fn write_output_program() {
        let program_content = include_str!("../cairo_programs/bitwise_output.json").as_bytes();
        let mut hint_processor = BuiltinHintProcessor::new_empty();
        let (mut cairo_runner, mut vm) = run_test_program(program_content, &mut hint_processor)
            .expect("Couldn't initialize cairo runner");

        let mut buffer = String::new();
        write_output(&mut cairo_runner, &mut vm, &mut buffer).unwrap();
        assert_eq!(&buffer, "0\n");
    }

    #[test]
    #[cfg(feature = "std")]
    fn write_binary_trace_file() {
        let program_content = std::fs::read(Path::new("cairo_programs/struct.json")).unwrap();
        let expected_trace_path = Path::new("cairo_programs/trace_memory/cairo_trace_struct");
        let cairo_rs_trace_path = Path::new("cairo_programs/trace_memory/struct_cairo_rs.trace");

        // run test program until the end
        let mut hint_processor = BuiltinHintProcessor::new_empty();
        let cairo_runner_result = run_test_program(&program_content, &mut hint_processor);
        let (mut cairo_runner, mut vm) = cairo_runner_result.unwrap();

        // relocate memory so we can dump it to file
        assert!(cairo_runner.relocate(&mut vm).is_ok());
        assert!(vm.trace.is_some());
        assert!(cairo_runner.relocated_trace.is_some());

        // write cairo_rs vm trace file
        assert!(
            write_binary_trace(&cairo_runner.relocated_trace.unwrap(), cairo_rs_trace_path).is_ok()
        );

        // compare that the original cairo vm trace file and cairo_rs vm trace files are equal
        assert!(compare_files(cairo_rs_trace_path, expected_trace_path).is_ok());
    }

    #[test]
    #[cfg(feature = "std")]
    fn write_binary_memory_file() {
        let program_content = std::fs::read(Path::new("cairo_programs/struct.json")).unwrap();
        let expected_memory_path = Path::new("cairo_programs/trace_memory/cairo_memory_struct");
        let cairo_rs_memory_path = Path::new("cairo_programs/trace_memory/struct_cairo_rs.memory");

        // run test program until the end
        let mut hint_processor = BuiltinHintProcessor::new_empty();
        let cairo_runner_result = run_test_program(&program_content, &mut hint_processor);
        let (mut cairo_runner, mut vm) = cairo_runner_result.unwrap();

        // relocate memory so we can dump it to file
        assert!(cairo_runner.relocate(&mut vm).is_ok());

        // write cairo_rs vm memory file
        assert!(write_binary_memory(&cairo_runner.relocated_memory, cairo_rs_memory_path).is_ok());

        // compare that the original cairo vm memory file and cairo_rs vm memory files are equal
        assert!(compare_files(cairo_rs_memory_path, expected_memory_path).is_ok());
    }

    #[test]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
    fn run_with_no_trace() {
        let program = load_program!("../cairo_programs/struct.json", Some("main"));

        let mut hint_processor = BuiltinHintProcessor::new_empty();
        let mut cairo_runner = cairo_runner!(program);
        let mut vm = vm!();
        let end = cairo_runner.initialize(&mut vm).unwrap();
        assert!(cairo_runner
            .run_until_pc(end, &mut vm, &mut hint_processor)
            .is_ok());
        assert!(vm.trace.is_none());
    }
}
