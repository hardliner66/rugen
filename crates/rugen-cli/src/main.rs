use std::{path::PathBuf, sync::Arc};

use clap::{Parser, Subcommand};
use rugen::{
    module,
    rune::{
        Diagnostics, Source, Sources, Vm,
        alloc::clone::TryClone,
        termcolor::{ColorChoice, StandardStream},
    },
};

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run a Rune script to generate data
    Gen {
        #[arg(short, long)]
        pretty: bool,
        script: PathBuf,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Format a rune script
    Format { script: PathBuf },
}

fn generate(pretty: bool, script: &PathBuf, output: Option<PathBuf>) -> anyhow::Result<()> {
    let mut context = rune_modules::default_context()?;
    let mut sources = Sources::new();
    let source = Source::from_path(script)?;
    context.install(module(source.try_clone()?)?)?;
    sources.insert(source)?;
    let mut diagnostics = Diagnostics::new();

    let result = rugen::rune::prepare(&mut sources)
        .with_context(&context)
        .with_diagnostics(&mut diagnostics)
        .build();

    if !diagnostics.is_empty() {
        let mut writer = StandardStream::stderr(ColorChoice::Always);
        diagnostics.emit(&mut writer, &sources)?;
    }

    let unit = Arc::new(result?);
    let runtime = Arc::new(context.runtime()?);

    let mut vm = Vm::new(runtime.clone(), unit);

    let result = vm.call(rugen::rune::Hash::type_hash(["main"]), ())?;
    let output_string = if let Ok(string_result) = rugen::rune::from_value::<String>(&result) {
        string_result
    } else {
        let value = rugen::generate(result)?;
        if pretty {
            serde_json::to_string_pretty(&value)?
        } else {
            serde_json::to_string(&value)?
        }
    };
    if let Some(output_path) = output {
        std::fs::write(output_path, output_string)?;
    } else {
        println!("{output_string}");
    }
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let Cli { command } = Cli::parse();
    match command {
        Command::Gen {
            pretty,
            script,
            output,
        } => generate(pretty, &script, output),

        Command::Format { script } => {
            rugen::format_rune_script(&script).expect("Could not format script!");
            Ok(())
        }
    }
}
