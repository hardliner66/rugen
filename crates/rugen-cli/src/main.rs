use std::{path::PathBuf, sync::Arc};

use anyhow::bail;
use clap::Parser;
use rugen::{DataDescription, module};
use rune::{
    Diagnostics, Source, Sources, Vm,
    termcolor::{ColorChoice, StandardStream},
};

#[derive(Parser)]
struct Cli {
    #[arg(short, long)]
    pretty: bool,
    input: PathBuf,
    #[arg(short, long)]
    output: Option<PathBuf>,
}

fn main() -> anyhow::Result<()> {
    let Cli {
        input,
        output,
        pretty,
    } = Cli::parse();
    let mut context = rune_modules::default_context()?;
    context.install(module()?)?;
    let mut sources = Sources::new();
    sources.insert(Source::from_path(input)?)?;
    let mut diagnostics = Diagnostics::new();

    let result = rune::prepare(&mut sources)
        .with_context(&context)
        .with_diagnostics(&mut diagnostics)
        .build();

    if !diagnostics.is_empty() {
        let mut writer = StandardStream::stderr(ColorChoice::Always);
        diagnostics.emit(&mut writer, &sources)?;

        bail!("Script compilation failed");
    }

    let unit = Arc::new(result?);
    let runtime = Arc::new(context.runtime()?);

    let mut vm = Vm::new(runtime.clone(), unit);

    let result = vm.call(rune::Hash::type_hash(["main"]), ())?;
    let output_string = if let Ok(string_result) = rune::from_value::<String>(&result) {
        string_result
    } else {
        let description = DataDescription::from(&result);
        let value = description.generate()?;
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
