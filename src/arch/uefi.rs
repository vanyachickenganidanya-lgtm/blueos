use core::ptr::null_mut;

use crate::uefi::{GraphicsOutput, GopModeInfo, Guid, SystemTable, GOP_GUID, SUCCESS};

static mut SYSTEM_TABLE: *mut SystemTable = null_mut();

pub unsafe fn initialize(system_table: *mut SystemTable) {
    SYSTEM_TABLE = system_table;
}

pub fn system_table() -> *mut SystemTable {
    unsafe { SYSTEM_TABLE }
}

pub fn debug_write(text: &str) {
    unsafe {
        let table = SYSTEM_TABLE;
        if table.is_null() || (*table).con_out.is_null() {
            return;
        }
        let output = (*table).con_out;
        for byte in text.bytes() {
            if byte == b'\n' {
                let carriage = [b'\r' as u16, 0];
                ((*output).output_string)(output, carriage.as_ptr());
            }
            let character = [byte as u16, 0];
            ((*output).output_string)(output, character.as_ptr());
        }
    }
}

pub fn emergency_write(text: &str) {
    debug_write(text);
}

pub fn idle() {
    unsafe {
        let table = SYSTEM_TABLE;
        if !table.is_null() && !(*table).boot_services.is_null() {
            ((*(*table).boot_services).stall)(1_000);
        } else {
            core::hint::spin_loop();
        }
    }
}

pub unsafe fn locate_protocol<T>(guid: &Guid) -> Option<*mut T> {
    let table = SYSTEM_TABLE;
    if table.is_null() || (*table).boot_services.is_null() {
        return None;
    }
    let mut interface = null_mut();
    let status = ((*(*table).boot_services).locate_protocol)(guid, core::ptr::null(), &mut interface);
    if status == SUCCESS && !interface.is_null() {
        Some(interface.cast())
    } else {
        None
    }
}

pub unsafe fn framebuffer() -> Option<crate::framebuffer::Framebuffer> {
    let gop = locate_protocol::<GraphicsOutput>(&GOP_GUID)?;
    let boot_services = (*SYSTEM_TABLE).boot_services;
    let mode = (*gop).mode;
    if mode.is_null() {
        return None;
    }

    let mut preferred = (*mode).mode;
    let mut preferred_score = 0u64;
    for index in 0..(*mode).max_mode {
        let mut size = 0usize;
        let mut info: *mut GopModeInfo = null_mut();
        let status = ((*gop).query_mode)(gop, index, &mut size, &mut info);
        if status != SUCCESS || info.is_null() {
            continue;
        }
        let value = *info;
        let usable = value.pixel_format <= 1
            && value.horizontal_resolution >= 800
            && value.vertical_resolution >= 600;
        if usable {
            let exact = value.horizontal_resolution == 1024 && value.vertical_resolution == 768;
            let area = value.horizontal_resolution as u64 * value.vertical_resolution as u64;
            let score = if exact { u64::MAX } else { area.min(1_920 * 1_080) };
            if score > preferred_score {
                preferred = index;
                preferred_score = score;
            }
        }
        ((*boot_services).free_pool)(info.cast());
    }

    if preferred != (*mode).mode && ((*gop).set_mode)(gop, preferred) != SUCCESS {
        return None;
    }
    let mode = (*gop).mode;
    let info = (*mode).info;
    if info.is_null() || (*mode).framebuffer_base == 0 || (*info).pixel_format > 1 {
        return None;
    }
    let address = (*mode).framebuffer_base as usize;
    let width = (*info).horizontal_resolution as usize;
    let height = (*info).vertical_resolution as usize;
    let pitch = (*info).pixels_per_scan_line as usize * 4;
    if (*info).pixel_format == 0 {
        Some(crate::framebuffer::Framebuffer::new_rgb(address, width, height, pitch, 32))
    } else {
        Some(crate::framebuffer::Framebuffer::new(address, width, height, pitch, 32))
    }
}
