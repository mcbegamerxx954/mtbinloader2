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
        let _handle = protect_with_handle(plt_fn_ptr, PTR_LEN, Protection::READ_WRITE).unwrap();
        // Replace the function address
        plt_fn_ptr.write_unaligned(replacement);
    }
}
