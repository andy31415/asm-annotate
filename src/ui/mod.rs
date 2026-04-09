// TODO: Port rendering functions here
use crate::core::RenderGroup;
use color_eyre::eyre::Result;

pub fn render_unified(
    func_name: &str,
    groups: &[RenderGroup],
    show_stats: bool,
    show_bytes: bool,
) -> Result<()> {
    println!("Rendering unified for {}: TODO", func_name);
    Ok(())
}

pub fn render_split(
    func_name: &str,
    groups: &[RenderGroup],
    show_stats: bool,
    show_bytes: bool,
    src_width: usize,
) -> Result<()> {
    println!("Rendering split for {}: TODO", func_name);
    Ok(())
}

pub fn render_stats_table(groups: &[RenderGroup]) -> Result<()> {
    println!("Rendering stats table: TODO");
    Ok(())
}
