// TODO: Port rendering functions here
use crate::core::RenderGroup;
use color_eyre::eyre::Result;

pub fn render_unified(
    func_name: &str,
    _groups: &[RenderGroup],
    _show_stats: bool,
    _show_bytes: bool,
) -> Result<()> {
    println!("Rendering unified for {}: TODO", func_name);
    Ok(())
}

pub fn render_split(
    func_name: &str,
    _groups: &[RenderGroup],
    _show_stats: bool,
    _show_bytes: bool,
    _src_width: usize,
) -> Result<()> {
    println!("Rendering split for {}: TODO", func_name);
    Ok(())
}

pub fn render_stats_table(_groups: &[RenderGroup]) -> Result<()> {
    println!("Rendering stats table: TODO");
    Ok(())
}
