use crate::backends::demangle::DemanglerBackend;
use crate::backends::elf::FunctionInfo;
use color_eyre::eyre::{Result, eyre};
use skim::prelude::*;
use std::borrow::Cow;
use std::sync::Arc;

pub trait PickerBackend {
    fn pick_function(
        &self,
        functions: Vec<FunctionInfo>,
        demangler: &impl DemanglerBackend,
    ) -> Result<Option<FunctionInfo>>;
}

pub struct SkimBackend;

struct SkimItemWrapper {
    display_text: String,
    func: FunctionInfo,
}

impl SkimItem for SkimItemWrapper {
    fn text(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.display_text)
    }

    // We can add more methods here if we want to customize the preview, etc.
}

impl PickerBackend for SkimBackend {
    fn pick_function(
        &self,
        functions: Vec<FunctionInfo>,
        demangler: &impl DemanglerBackend,
    ) -> Result<Option<FunctionInfo>> {
        if functions.is_empty() {
            return Ok(None);
        }

        let items: Vec<SkimItemWrapper> = functions
            .into_iter()
            .map(|func| {
                let demangled = demangler
                    .demangle(&func.name)
                    .unwrap_or_else(|_| func.name.clone());
                let display_name = if demangled != func.name {
                    format!("{}  [{}]", demangled, func.name)
                } else {
                    func.name.clone()
                };
                let display_text = format!("{} ({}) {:#x}", display_name, func.size, func.addr);
                SkimItemWrapper { display_text, func }
            })
            .collect();

        let options = SkimOptionsBuilder::default()
            .multi(false)
            .build()
            .map_err(|e| eyre!("Failed to build Skim options: {}", e))?;

        let (tx, rx): (SkimItemSender, SkimItemReceiver) = unbounded();
        for item in items {
            let _ = tx.send(Arc::new(item));
        }
        drop(tx); // Close the channel

        let output = Skim::run_with(&options, Some(rx));

        match output {
            Some(out) => {
                if out.is_abort {
                    return Ok(None); // User aborted
                }
                let selected_items = out.selected_items;
                if selected_items.is_empty() {
                    Ok(None) // No selection
                } else {
                    // Since multi is false, we expect at most one item
                    let selected_item = selected_items[0].clone();
                    let wrapper = (*selected_item)
                        .as_any()
                        .downcast_ref::<SkimItemWrapper>()
                        .ok_or_else(|| eyre!("Failed to downcast SkimItem"))?;
                    Ok(Some(wrapper.func.clone()))
                }
            }
            None => Ok(None), // Should not happen in this configuration
        }
    }
}
