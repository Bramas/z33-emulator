#![forbid(unsafe_code)]
#![deny(clippy::all, clippy::pedantic)]
#![allow(clippy::missing_errors_doc, clippy::missing_panics_doc)]

use std::collections::HashMap;
use std::path::PathBuf;

use serde::Serialize;
use wasm_bindgen::prelude::*;

use z33_emulator::{
    compile,
    parse,
    compiler::CompilationError,
    runtime::ProcessorError,
    compiler::layout,
    runtime::Exception::HardwareInterrupt,
    constants as C,
    parser::location::{AbsoluteLocation, MapLocation},
    preprocessor::{InMemoryFilesystem, Preprocessor},
    runtime::Registers,
    runtime::Computer,
};
use codespan_reporting::files::SimpleFiles;
use codespan_reporting::diagnostic::{Diagnostic, Label};

#[derive(Default, Serialize)]
struct Output {
    ast: Option<String>,
    preprocessed: Option<String>,
    memory: Vec<(C::Address, String)>,
    labels: HashMap<String, C::Address>,
    error: Option<String>,
    registers: Option<String>,
}

fn computer_steps(computer: &mut Computer, steps: u32) -> (Vec<String>, Result<(), ProcessorError>) {
    let mut instructions = Vec::<String>::new();

    for _ in 0..steps {
        instructions.push(format!("{:?}", computer.next_instruction().unwrap_or(String::from("Invalid instruction"))));
        match computer.step() {
            Ok(_) => {}
            Err(ProcessorError::Reset) => return (instructions, Ok(())),
            Err(v) => return (instructions, Err(v)),
        }
    };
    (instructions, Err(ProcessorError::Exception(HardwareInterrupt)))
}

fn char_offset(a: &str, b: &str) -> usize {
    let a = a.as_ptr();
    let b = b.as_ptr();
    b as usize - a as usize
}

#[wasm_bindgen]
pub fn dump(source: &str) -> Result<JsValue, JsValue> {
    let mut output = Output::default();
    let mut files = HashMap::new();
    let path = PathBuf::from("-");
    files.insert(path.clone(), source.to_string());

    let fs = InMemoryFilesystem::new(files);
    let preprocessor = Preprocessor::new(fs).and_load(&path);

    let source = match preprocessor.preprocess(&path) {
        Ok(s) => s,
        Err(e) => {
            output.error = Some(format!("{e}"));
            return Ok(serde_wasm_bindgen::to_value(&output)?);
        }
    };

    output.preprocessed = Some(source.clone());
    let source = source.as_str();
    let mut files = SimpleFiles::new();
    let file_id = files.add("preprocessed", source);

    let program = parse(&source); // TODO: the error is tied to the input


    let program = match program {
        Ok(p) => p,
        Err(e) => {

            let msg = format!("{e}");
            let labels: Vec<_> = e
                .errors
                .iter()
                .map(|(location, kind)| {
                    let message = match kind {
                        nom::error::VerboseErrorKind::Context(s) => (*s).to_owned(),
                        nom::error::VerboseErrorKind::Char(c) => format!("expected '{c}'"),
                        nom::error::VerboseErrorKind::Nom(code) => format!("{code:?}"),
                    };
                    let offset = char_offset(source, location);

                    Label::primary(file_id, offset..offset).with_message(message)
                })
                .collect();
            let diagnostic = Diagnostic::error().with_message(msg).with_labels(labels);

            let config = codespan_reporting::term::Config {
                before_label_lines: 3,
                after_label_lines: 3,
                ..Default::default()
            };
            let mut buf = [0u8; 1024];
            let mut bufWrt =  codespan_reporting::term::termcolor::Ansi::new(&mut buf as &mut [u8]);
            codespan_reporting::term::emit(&mut bufWrt, &config, &files, &diagnostic);

            output.error = Some(format!("{}",
            std::str::from_utf8(&buf).unwrap()));
            return Ok(serde_wasm_bindgen::to_value(&output)?);
        }
    };

    let ast = program.to_node();

    // Transform the AST relative locations to absolute ones
    let ast = ast.map_location(&AbsoluteLocation::default());

    output.ast = Some(format!("{ast}"));

    /*let layout = layout(program.inner);

    if let Err(e) = layout {
        output.error = Some(format!("{e}"));
        return Ok(serde_wasm_bindgen::to_value(&output)?);
    }

    let layout = layout.unwrap();
    output.memory = layout.memory_report();
    output.labels = layout.labels;
*/

    let parent = AbsoluteLocation::<()>::default();
    let program = program.map_location(&parent);

    let (mut computer, debug_info) = match compile(program.inner, "main") {
        Ok(p) => p,
        Err(e) => {
            let mut last_error = &e as &dyn std::error::Error;
            for error in anyhow::Chain::new(&e) {
                // TODO: get the location of individual errors
                //error!("{}", error);
                last_error = error;
            }

            let msg = format!("{last_error}");
            let location = match &e {
                CompilationError::MemoryLayout(e) => e.location(),
                CompilationError::MemoryFill(e) => Some(e.location()),
                CompilationError::UnknownEntrypoint(_e) => {
                    output.error = Some(format!("\u{1b}[0m\u{1b}[1m\u{1b}[38;5;9merror\u{1b}[0m: Unable to find entrypoint 'main'"));
                    return Ok(serde_wasm_bindgen::to_value(&output)?);
                },
            };
            if let Some(location) = location {
                let label = Label::primary(
                    file_id,
                    location.offset..(location.offset + location.length),
                );

                let diagnostic = Diagnostic::error()
                    .with_message(msg)
                    .with_labels(vec![label]);

                let mut buf = [0u8; 1024];
                let mut bufWrt =  codespan_reporting::term::termcolor::Ansi::new(&mut buf as &mut [u8]);
                let config = codespan_reporting::term::Config {
                    before_label_lines: 3,
                    after_label_lines: 3,
                    ..Default::default()
                };

                codespan_reporting::term::emit(
                    &mut bufWrt,
                    &config,
                    &files,
                    &diagnostic,
                );
                output.error = Some(format!("{}",
                std::str::from_utf8(&buf).unwrap()));
                return Ok(serde_wasm_bindgen::to_value(&output)?);
            }

            output.error = Some(format!("{e:#?}"));
            return Ok(serde_wasm_bindgen::to_value(&output)?);
        }
    };
    let (steps, status) = computer_steps(&mut computer, 10000);

    match status {
        Ok(()) => {},
        Err(e) => {
            output.error = Some(format!("{e:#?}\n\n Instructions:\n{}", steps.join("\n")));
            return Ok(serde_wasm_bindgen::to_value(&output)?);
        }
    };

    let mut memory = Vec::new();
    for i in (9980..10000).rev() {
        match computer.memory.get(i) {
            Ok(value) => match value {
                //Empty => break,
                _ => memory.push(format!("{}: {:?}", i, value)),
            },
            Err(_) => { 
                memory.push(format!("Err"));  //break
            },
        }
    }
    output.registers = Some(format!("<b><span style=\"color:#35cc5d\">Execution: OK</span></b>\n\n{:?}\n\n{}", computer.registers,
    memory.join("\n")));

    Ok(serde_wasm_bindgen::to_value(&output)?)
}
