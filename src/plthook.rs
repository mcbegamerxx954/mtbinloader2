use libc::c_void;
use plt_rs::DynamicLibrary;

pub fn replace_plt_functions(dyn_lib: &DynamicLibrary, functions: &[(&str, *const ())]) {
    let base_addr = dyn_lib.library().addr();
    for (fn_name, replacement) in functions {
        let Some(fn_plt) = dyn_lib.try_find_function(fn_name) else {
            //            log::warn!("Missing symbol: {fn_name}");
            continue;
        };
        //        log::info!("Hooking {}...", fn_name);
        replace_plt_function(base_addr, fn_plt.r_offset as usize, *replacement);
    }
    //    log::info!("Hooked {} functions.", functions.len());
}
fn replace_plt_function(base_addr: usize, offset: usize, replacement: *const ()) {
    let plt_fn_ptr = (base_addr + offset) as *mut *mut c_void;
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGE_SIZE) as usize };
    let plt_page = ((plt_fn_ptr as usize / page_size) * page_size) as *mut c_void;
    unsafe {
        // Set the memory page to read, write
        let prot_res = libc::mprotect(plt_page, page_size, libc::PROT_WRITE | libc::PROT_READ);
        if prot_res != 0 {
            println!("Protection edit result: {prot_res}");
            // return Err(HookError::OsError(
            //     "Mprotect error on setting rw".to_string(),
            // ));
            unreachable!();
        }

        // Replace the function address
        let _prev_addr = std::ptr::replace(plt_fn_ptr, replacement as *mut _);

        // Set the memory page protection back to read only
        let prot_res = libc::mprotect(plt_page, page_size, libc::PROT_READ);
        if prot_res != 0 {
            // return Err(HookError::OsError(
            //     "Mprotect error on setting read only".to_string(),
            // ));
            unreachable!();
        }
    }
}
