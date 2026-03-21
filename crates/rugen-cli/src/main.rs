use std::{path::PathBuf, sync::Arc};

use clap::{Parser, Subcommand};
use rugen::{
    DescriptionError, fix_line_number, module,
    rune::{
        Diagnostics, Source, SourceId, Sources, Value, Vm,
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
    context.install(module()?)?;
    let mut sources = Sources::new();
    sources.insert(Source::from_path(script)?)?;
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
        let value = if let Ok(value) =
            rugen::rune::from_value::<Result<Value, DescriptionError>>(&result)
        {
            value.map_err(|e| fix_line_number(e, &sources.get(SourceId::new(0)).unwrap()))?
        } else {
            result
        };
        // {
        //     result?
        // } else {
        //     rugen::checked_from_value::<Value>(&result)?
        // };
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
