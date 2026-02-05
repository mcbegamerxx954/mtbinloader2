#[cfg(target_pointer_width = "64")]
use std::{borrow::Cow, collections::HashMap};

#[cfg(target_pointer_width = "64")]
use plt_rs::elf64::{self, DynRela};
use plt_rs::DynamicLibrary;
use region::{protect_with_handle, Protection};

pub fn replace_plt_functions<const LEN: usize>(
    dyn_lib: &DynamicLibrary,
    functions: [(&str, *const u8); LEN],
) {
    let base_addr = dyn_lib.library().addr();
    for (fn_name, replacement) in functions {
        let Some(fn_plt) = dyn_lib.try_find_function(fn_name) else {
            continue;
        };
        replace_plt_function(base_addr, fn_plt.r_offset as usize, replacement);
    }
}
fn replace_plt_function(base_addr: usize, offset: usize, replacement: *const u8) {
    let plt_fn_ptr = (base_addr + offset) as *mut *const u8;
    const PTR_LEN: usize = std::mem::size_of::<usize>();
    unsafe {
        // Set the memory page to read, write
        let _handle = protect_with_handle(plt_fn_ptr, PTR_LEN, Protection::READ_WRITE)
            .expect("Mprotect failed");
        // Replace the function address
        plt_fn_ptr.write_unaligned(replacement);
    }
}

/// Finding target function differs on 32 bit and 64 bit.
/// On 32 bit we want to check the relocations table only, opposed to the addend relocations table.
/// Additionally, we will fall back to the plt given it is an addendless relocation table.
#[cfg(target_pointer_width = "32")]
pub fn try_find_function(&self, symbol_name: &str) -> Option<&'_ elf32::DynRel> {
    let string_table = self.string_table();
    let dyn_symbols = self.symbols()?;
    if let Some(dyn_relas) = self.relocs() {
        let dyn_relas = dyn_relas.entries().iter();
        if let Some(symbol) = dyn_relas
            .flat_map(|e| {
                dyn_symbols
                    .resolve_name(e.symbol_index() as usize, string_table)
                    .map(|s| (e, s))
            })
            .filter(|(_, s)| s.eq(symbol_name))
            .next()
            .map(|(target_function, _)| target_function)
        {
            return Some(symbol);
        }
    }

    if let Some(dyn_relas) = self.plt_rel() {
        let dyn_relas = dyn_relas.entries().iter();
        if let Some(symbol) = dyn_relas
            .flat_map(|e| {
                dyn_symbols
                    .resolve_name(e.symbol_index() as usize, string_table)
                    .map(|s| (e, s))
            })
            .filter(|(_, s)| s.eq(symbol_name))
            .next()
            .map(|(target_function, _)| target_function)
        {
            return Some(symbol);
        }
    }
    None
}

/// Finding target function differs on 32 bit and 64 bit.
/// On 64 bit we want to check the addended relocations table only, opposed to the addendless relocations table.
/// Additionally, we will fall back to the plt given it is an addended relocation table.
#[cfg(target_pointer_width = "64")]
pub fn get_function_table<'a>(
    dnlib: &'a DynamicLibrary,
) -> Option<HashMap<Cow<'a, str>, &'a DynRela>> {
    let string_table = dnlib.string_table();
    let symbols = dnlib.symbols()?;
    let mut hashmap = HashMap::new();
    if let Some(dyn_relas) = dnlib.addend_relocs() {
        hashmap.reserve(dyn_relas.entries().len());
        let dyn_relas = dyn_relas.entries().iter();
        for entry in dyn_relas {
            let Some(name) = symbols.resolve_name(entry.symbol_index() as usize, &string_table)
            else {
                continue;
            };
            hashmap.insert(name, entry);
        }
    }

    if let Some(dyn_relas) = dnlib.plt_rela() {
        hashmap.reserve(dyn_relas.entries().len());

        let dyn_relas = dyn_relas.entries().iter();
        let dyn_relas = dyn_relas.entries().iter();
        for entry in dyn_relas {
            let Some(name) = symbols.resolve_name(entry.symbol_index() as usize, &string_table)
            else {
                continue;
            };
            hashmap.insert(name, entry);
        }
    }
    if hashmap.is_empty() {
        None
    } else {
        Some(hashmap)
    }
}
