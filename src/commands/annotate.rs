use crate::cli::AnnotateArgs;
use color_eyre::eyre::Result;

pub fn handle_annotate(args: &AnnotateArgs) -> Result<()> {
    println!("Annotating function in: {}", args.elf.display());
    // TODO: Implement annotate functionality
    Ok(())
}
