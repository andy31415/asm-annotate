use crate::backends::elf::FunctionInfo;
use color_eyre::eyre::Result;

pub trait PickerBackend {
    fn pick_function(&self, functions: Vec<FunctionInfo>) -> Result<Option<FunctionInfo>>;
}

// TODO: Implement SkimBackend
pub struct SkimBackend;

impl PickerBackend for SkimBackend {
    fn pick_function(&self, _functions: Vec<FunctionInfo>) -> Result<Option<FunctionInfo>> {
        unimplemented!()
    }
}
