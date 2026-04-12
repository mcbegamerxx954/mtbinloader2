// #[cfg(target_pointer_width = "64")]
use std::{borrow::Cow, collections::HashMap};

// #[cfg(target_pointer_width = "64")]
// use plt_rs::elf64::{self, DynRela};
use plt_rs::DynamicLibrary;
use region::{protect, protect_with_handle, Protection};

pub fn replace_plt_functions<const LEN: usize>(
    dyn_lib: &DynamicLibrary,
    functions: [(&str, *const u8); LEN],
) {
    let base_addr = dyn_lib.library().addr();
    let Some(table) = get_function_table(dyn_lib) else {
        log::warn!("Wtf");
        return;
    };
    for (fn_name, replacement) in functions {
        let name = Cow::Borrowed(fn_name);
        if let Some(fn_plt) = table.get(&name) {
            replace_plt_function(base_addr, fn_plt.r_offset as usize, replacement);
        }
    }
}
fn replace_plt_function(base_addr: usize, offset: usize, replacement: *const u8) {
    let plt_fn_ptr = (base_addr + offset) as *mut *const u8;
    const PTR_LEN: usize = std::mem::size_of::<usize>();
    unsafe {
        // Set the memory page to read, write
        let _handle =
            protect(plt_fn_ptr, PTR_LEN, Protection::READ_WRITE).expect("Mprotect failed");
        // Replace the function address
        plt_fn_ptr.write_unaligned(replacement);
        protect(plt_fn_ptr, PTR_LEN, Protection::READ_EXECUTE).unwrap();
    }
}

// /// Finding target function differs on 32 bit and 64 bit.
// /// On 32 bit we want to check the relocations table only, opposed to the addend relocations table.
// /// Additionally, we will fall back to the plt given it is an addendless relocation table.

macro_rules! collect_entries {
    ($iter:ident,$syms:expr,$table:expr) => {
        $iter
            .entries()
            .iter()
            .map(|e| {
                $syms
                    .resolve_name(e.symbol_index() as usize, $table)
                    .map(|s| (s, e))
            })
            .flatten()
        //            .collect()
    };
}

#[cfg(target_pointer_width = "32")]
pub fn get_function_table<'a>(
    dnlib: &'a DynamicLibrary,
) -> Option<HashMap<Cow<'a, str>, &'a plt_rs::elf32::DynRel>> {
    let string_table = dnlib.string_table();
    let dyn_symbols = dnlib.symbols()?;
    let mut hashmap = HashMap::new();
    if let Some(dyn_relas) = dnlib.relocs() {
        let iter = collect_entries!(dyn_relas, dyn_symbols, string_table);
        hashmap.extend(iter);
    }

    if let Some(dyn_relas) = dnlib.plt_rel() {
        let iter = collect_entries!(dyn_relas, dyn_symbols, string_table);
        hashmap.extend(iter);
    }
    None
}

// /// Finding target function differs on 32 bit and 64 bit.
// /// On 64 bit we want to check the addended relocations table only, opposed to the addendless relocations table.
// /// Additionally, we will fall back to the plt given it is an addended relocation table.
#[cfg(target_pointer_width = "64")]
pub fn get_function_table<'a>(
    dnlib: &'a DynamicLibrary,
) -> Option<HashMap<Cow<'a, str>, &'a plt_rs::elf64::DynRela>> {
    let string_table = dnlib.string_table();
    let symbols = dnlib.symbols()?;
    let mut hashmap = HashMap::new();
    if let Some(dyn_relas) = dnlib.addend_relocs() {
        let entries = collect_entries!(dyn_relas, &symbols, &string_table);
        hashmap.extend(entries);
    }
    if let Some(dyn_relas) = dnlib.plt_rela() {
        let entries = collect_entries!(dyn_relas, &symbols, &string_table);
        hashmap.extend(entries);
    }
    if hashmap.is_empty() {
        None
    } else {
        Some(hashmap)
    }
}
