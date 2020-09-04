use std::fs::File;
use std::io::Read;
use std::path::PathBuf;
use structopt::StructOpt;
use tracing::info;

mod compiler;
mod parser;
mod processor;
mod util;

use crate::compiler::{Compiler, CompilerState};
use crate::parser::{Parser, ProgramLine};

#[derive(StructOpt)]
struct Opt {
    /// Input file
    #[structopt(parse(from_os_str))]
    input: PathBuf,

    /// Start label
    entrypoint: String,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .compact()
        .init();

    let opt = Opt::from_args();
    info!("Reading program from file {:?}", opt.input);

    let mut file = File::open(opt.input)?;
    let mut source = String::new();
    file.read_to_string(&mut source)?;

    info!("Parsing program");
    let parser = Parser::new(source.as_str());

    let mut compiler = CompilerState::default();
    for line in parser {
        let line = line?;
        match line {
            ProgramLine::Instruction(inst) => {
                compiler.ingest(inst)?;
            }
            ProgramLine::LabeledInstruction(label, inst) => {
                compiler.ingest_labeled_instruction(inst, label)?;
            }
            ProgramLine::Label(label) => {
                compiler.ingest_label(label)?;
            }
            ProgramLine::Empty => {}
        }
    }

    info!("Buiding computer (entrypoint: {})", opt.entrypoint);
    let mut computer = compiler.build(opt.entrypoint)?;
    info!("Running program");
    computer.run()?;

    println!("Registers:");
    util::display_registers(computer.registers);
    println!("-------");
    println!("Memory:");
    util::display_memory(computer.memory);

    Ok(())
}
