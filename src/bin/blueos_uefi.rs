#![no_std]
#![no_main]

#[path = "../app.rs"]
mod app;
#[path = "../arch/uefi.rs"]
mod arch;
#[path = "../font.rs"]
mod font;
#[path = "../framebuffer.rs"]
mod framebuffer;
#[path = "../lua.rs"]
mod lua;
#[path = "../net.rs"]
mod net;
#[path = "../shell.rs"]
mod shell;
#[path = "../uefi.rs"]
mod uefi;

use core::{ffi::c_void, panic::PanicInfo, ptr::null_mut};

use framebuffer::{Ui, UiAction};
use net::{NetworkStack, Nic};
use shell::{FeedResult, Shell};
use uefi::{
    BlockIo, Handle, InputKey, SimpleNetwork, SimplePointer, SimplePointerState, Status,
    SystemTable, Time, BLOCK_IO_GUID, BY_PROTOCOL, LOADER_DATA, SIMPLE_NETWORK_GUID,
    SIMPLE_POINTER_GUID, SUCCESS,
};

const IMAGE_BLOCKS: u64 = 131_072; // 64 MiB hybrid BIOS/UEFI image.

const SNP_TX_COUNT: usize = 4;
const SNP_BUFFER_SIZE: usize = 1_600;

struct FirmwareNic {
    protocol: *mut SimpleNetwork,
    mac: [u8; 6],
    transmit_buffers: [[u8; SNP_BUFFER_SIZE]; SNP_TX_COUNT],
    transmit_busy: [bool; SNP_TX_COUNT],
}

impl FirmwareNic {
    unsafe fn probe() -> Option<Self> {
        let protocol = arch::locate_protocol::<SimpleNetwork>(&SIMPLE_NETWORK_GUID)?;
        let mut mode = (*protocol).mode;
        if mode.is_null() {
            return None;
        }
        if (*mode).state == 0 && uefi::is_error(((*protocol).start)(protocol)) {
            return None;
        }
        mode = (*protocol).mode;
        if mode.is_null() {
            return None;
        }
        if (*mode).state == 1
            && uefi::is_error(((*protocol).initialize)(protocol, 0, 0))
        {
            return None;
        }
        mode = (*protocol).mode;
        if mode.is_null()
            || (*mode).state != 2
            || (*mode).hardware_address_size < 6
            || ((*mode).media_present_supported != 0 && (*mode).media_present == 0)
        {
            return None;
        }
        let mut mac = [0u8; 6];
        mac.copy_from_slice(&(*mode).current_address.address[..6]);
        if mac == [0; 6] {
            return None;
        }
        Some(Self {
            protocol,
            mac,
            transmit_buffers: [[0; SNP_BUFFER_SIZE]; SNP_TX_COUNT],
            transmit_busy: [false; SNP_TX_COUNT],
        })
    }

    fn reclaim(&mut self) {
        unsafe {
            for _ in 0..SNP_TX_COUNT {
                let mut interrupt_status = 0u32;
                let mut recycled: *mut c_void = null_mut();
                if uefi::is_error(((*self.protocol).get_status)(
                    self.protocol,
                    &mut interrupt_status,
                    &mut recycled,
                )) || recycled.is_null()
                {
                    break;
                }
                for index in 0..SNP_TX_COUNT {
                    if self.transmit_buffers[index].as_mut_ptr().cast::<c_void>() == recycled {
                        self.transmit_busy[index] = false;
                    }
                }
            }
        }
    }
}

impl Nic for FirmwareNic {
    fn mac_address(&self) -> [u8; 6] {
        self.mac
    }

    fn receive(&mut self, packet: &mut [u8]) -> Option<usize> {
        unsafe {
            let mut header_size = 0usize;
            let mut buffer_size = packet.len();
            let status = ((*self.protocol).receive)(
                self.protocol,
                &mut header_size,
                &mut buffer_size,
                packet.as_mut_ptr().cast(),
                null_mut(),
                null_mut(),
                null_mut(),
            );
            if status == SUCCESS {
                Some(buffer_size.min(packet.len()))
            } else {
                None
            }
        }
    }

    fn transmit(&mut self, packet: &[u8]) -> bool {
        if packet.len() > SNP_BUFFER_SIZE {
            return false;
        }
        self.reclaim();
        let index = match self.transmit_busy.iter().position(|busy| !*busy) {
            Some(index) => index,
            None => return false,
        };
        self.transmit_buffers[index][..packet.len()].copy_from_slice(packet);
        let status = unsafe {
            ((*self.protocol).transmit)(
                self.protocol,
                0,
                packet.len(),
                self.transmit_buffers[index].as_mut_ptr().cast(),
                null_mut(),
                null_mut(),
                null_mut(),
            )
        };
        if uefi::is_error(status) {
            false
        } else {
            self.transmit_busy[index] = true;
            true
        }
    }
}

#[derive(Clone, Copy)]
struct Installer {
    source: *mut BlockIo,
    targets: [*mut BlockIo; 8],
    target_count: usize,
    selected: usize,
}

impl Installer {
    const fn unavailable() -> Self {
        Self {
            source: null_mut(),
            targets: [null_mut(); 8],
            target_count: 0,
            selected: usize::MAX,
        }
    }

    unsafe fn discover(table: *mut SystemTable) -> Self {
        if table.is_null() || (*table).boot_services.is_null() {
            return Self::unavailable();
        }
        let services = (*table).boot_services;
        let mut count = 0usize;
        let mut handles: *mut Handle = null_mut();
        let status = ((*services).locate_handle_buffer)(
            BY_PROTOCOL,
            &BLOCK_IO_GUID,
            core::ptr::null(),
            &mut count,
            &mut handles,
        );
        if status != SUCCESS || handles.is_null() {
            return Self::unavailable();
        }

        let mut result = Self::unavailable();
        for index in 0..count {
            let mut interface: *mut c_void = null_mut();
            if ((*services).handle_protocol)(
                *handles.add(index),
                &BLOCK_IO_GUID,
                &mut interface,
            ) != SUCCESS || interface.is_null()
            {
                continue;
            }
            let block = interface.cast::<BlockIo>();
            let media = (*block).media;
            if media.is_null()
                || (*media).media_present == 0
                || (*media).logical_partition != 0
                || (*media).block_size != 512
            {
                continue;
            }
            if (*media).removable_media != 0 && result.source.is_null() {
                // Installation is deliberately disabled unless a whole,
                // removable source medium can be distinguished from targets.
                result.source = block;
            } else if (*media).removable_media == 0
                && (*media).read_only == 0
                && (*media).last_block.saturating_add(1) >= IMAGE_BLOCKS
                && result.target_count < result.targets.len()
            {
                result.targets[result.target_count] = block;
                result.target_count += 1;
            }
        }
        ((*services).free_pool)(handles.cast());
        result
    }

    fn show(&self, ui: &mut Ui) {
        ui.open_installer();
        unsafe {
            if self.source.is_null() {
                ui.write("Source: no removable BlueOS USB was found.\n");
            } else {
                ui.write("Source: removable live media, 64 MiB payload ready.\n");
            }
            if self.target_count == 0 {
                ui.write("Targets: no writable whole internal disk of at least 64 MiB.\n");
            } else {
                ui.write("Target candidates (selection is mandatory):\n");
                for index in 0..self.target_count {
                    let media = (*self.targets[index]).media;
                    let mib = (*media).last_block.saturating_add(1) / 2_048;
                    ui.write("  DISK ");
                    ui.write_number((index + 1) as i64);
                    ui.write(": ");
                    ui.write_number(mib as i64);
                    ui.write(" MiB, media ID ");
                    ui.write_number((*media).media_id as i64);
                    if self.selected == index {
                        ui.write("  [SELECTED]");
                    }
                    ui.write("\n");
                }
            }
        }
        if self.source.is_null() || self.target_count == 0 {
            ui.write("\nInstaller remains read-only. Check the media and reboot.\n> ");
        } else if self.selected >= self.target_count {
            ui.write("\nType INSTALL SELECT 1 (or another listed number).\n> ");
        } else {
            ui.write("\nDANGER: type INSTALL ERASE to overwrite only the selected disk.\n> ");
        }
    }

    fn select(&mut self, number: usize, ui: &mut Ui) {
        if number == 0 || number > self.target_count {
            ui.write("Installer: that disk number is not in the candidate list.\n> ");
            return;
        }
        self.selected = number - 1;
        self.show(ui);
    }

    fn target(&self) -> Option<*mut BlockIo> {
        if self.selected < self.target_count {
            Some(self.targets[self.selected])
        } else {
            None
        }
    }

    unsafe fn install(&self, table: *mut SystemTable, ui: &mut Ui) {
        let target = match self.target() {
            Some(target) if !self.source.is_null() => target,
            _ => {
                self.show(ui);
                return;
            }
        };
        let services = (*table).boot_services;
        let source_media = (*self.source).media;
        let target_media = (*target).media;
        let chunk_blocks = 128usize;
        let chunk_bytes = chunk_blocks * 512;
        let mut buffer: *mut c_void = null_mut();
        if ((*services).allocate_pool)(LOADER_DATA, chunk_bytes * 2, &mut buffer) != SUCCESS
            || buffer.is_null()
        {
            ui.write("Installer error: firmware could not allocate transfer buffers.\n> ");
            return;
        }
        let verify_buffer = (buffer as *mut u8).add(chunk_bytes).cast::<c_void>();

        ui.write("Writing BlueOS hybrid BIOS/UEFI image...\n");
        let mut lba = 0u64;
        let mut next_report = 0u64;
        let mut failed = false;
        while lba < IMAGE_BLOCKS {
            let blocks = ((IMAGE_BLOCKS - lba) as usize).min(chunk_blocks);
            let bytes = blocks * 512;
            let read = ((*self.source).read_blocks)(
                self.source,
                (*source_media).media_id,
                lba,
                bytes,
                buffer,
            );
            if uefi::is_error(read) {
                ui.write("Installer error: reading the live USB failed.\n");
                failed = true;
                break;
            }
            let write = ((*target).write_blocks)(
                target,
                (*target_media).media_id,
                lba,
                bytes,
                buffer,
            );
            if uefi::is_error(write) {
                ui.write("Installer error: writing the destination failed.\n");
                failed = true;
                break;
            }
            lba += blocks as u64;
            if lba >= next_report {
                ui.write("Write progress: ");
                ui.write_number((lba * 100 / IMAGE_BLOCKS) as i64);
                ui.write("%\n");
                next_report = lba + IMAGE_BLOCKS / 10;
            }
        }

        if !failed && uefi::is_error(((*target).flush_blocks)(target)) {
            ui.write("Installer error: firmware could not flush the destination.\n");
            failed = true;
        }

        if !failed {
            ui.write("Write complete. Starting independent read-back verification...\n");
            lba = 0;
            next_report = 0;
            while lba < IMAGE_BLOCKS {
                let blocks = ((IMAGE_BLOCKS - lba) as usize).min(chunk_blocks);
                let bytes = blocks * 512;
                let source_read = ((*self.source).read_blocks)(
                    self.source,
                    (*source_media).media_id,
                    lba,
                    bytes,
                    buffer,
                );
                let target_read = ((*target).read_blocks)(
                    target,
                    (*target_media).media_id,
                    lba,
                    bytes,
                    verify_buffer,
                );
                if uefi::is_error(source_read) || uefi::is_error(target_read) {
                    ui.write("Verification error: a disk read failed.\n");
                    failed = true;
                    break;
                }
                let expected = core::slice::from_raw_parts(buffer.cast::<u8>(), bytes);
                let actual = core::slice::from_raw_parts(verify_buffer.cast::<u8>(), bytes);
                if expected != actual {
                    ui.write("VERIFICATION FAILED at LBA ");
                    ui.write_number(lba as i64);
                    ui.write(". Do not boot the destination.\n");
                    failed = true;
                    break;
                }
                lba += blocks as u64;
                if lba >= next_report {
                    ui.write("Verify progress: ");
                    ui.write_number((lba * 100 / IMAGE_BLOCKS) as i64);
                    ui.write("%\n");
                    next_report = lba + IMAGE_BLOCKS / 10;
                }
            }
        }

        if !failed {
            ui.write("INSTALLATION VERIFIED. Remove the USB drive and reboot.\n");
        } else {
            ui.write("Installation did not complete safely. The target may be unusable.\n");
        }
        ((*services).free_pool)(buffer);
        ui.write("> ");
    }
}

fn open_action(action: UiAction, ui: &mut Ui, shell: &mut Shell, installer: &Installer) {
    match action {
        UiAction::Launcher => ui.open_launcher(),
        UiAction::Terminal => {
            ui.open_window("KONSOLE");
            ui.write("> ");
        }
        UiAction::Files => ui.open_files(),
        UiAction::Settings => ui.open_settings(),
        UiAction::Installer => installer.show(ui),
        UiAction::None => return,
    }
    shell.reset();
}

fn pointer_delta(value: i32) -> i32 {
    if value == 0 {
        0
    } else {
        let scaled = value / 8;
        if scaled == 0 { value.signum() } else { scaled.clamp(-48, 48) }
    }
}

#[no_mangle]
pub unsafe extern "efiapi" fn efi_main(
    image_handle: Handle,
    system_table: *mut SystemTable,
) -> Status {
    let _ = image_handle;
    arch::initialize(system_table);
    if system_table.is_null() || (*system_table).boot_services.is_null() {
        return 1;
    }
    ((*(*system_table).boot_services).set_watchdog_timer)(0, 0, 0, core::ptr::null());
    arch::debug_write("BlueOS AMD64 UEFI workspace entry\n");

    let framebuffer = match arch::framebuffer() {
        Some(framebuffer) => framebuffer,
        None => {
            arch::debug_write("UEFI GOP framebuffer unavailable\n");
            framebuffer::Framebuffer::unavailable()
        }
    };
    let mut ui = Ui::new(framebuffer, "AMD64 UEFI");
    ui.open_window("WELCOME TO BLUEOS");
    ui.write("BLUEOS PLASMA-STYLE LIVE WORKSPACE\n");
    ui.write("F1 APPLICATIONS  F2 TERMINAL  F3 FILES  F4 SETTINGS  F8 INSTALLER\n\n");
    app::run_lua_demo(&mut ui);
    ui.write("> ");

    let mut installer = Installer::discover(system_table);
    let mut shell = Shell::new();
    let mut network = FirmwareNic::probe().map(|mut nic| {
        let mut stack = NetworkStack::new(nic.mac_address());
        stack.start(&mut nic);
        (nic, stack)
    });
    if network.is_some() {
        ui.write("UEFI Simple Network Protocol online.\n> ");
    } else {
        ui.write("UEFI network protocol unavailable; desktop remains offline.\n> ");
    }
    let pointer = arch::locate_protocol::<SimplePointer>(&SIMPLE_POINTER_GUID);
    if let Some(device) = pointer {
        ((*device).reset)(device, 0);
    }
    let mut pointer_x = ui.framebuffer.width / 2;
    let mut pointer_y = ui.framebuffer.height / 2;
    if pointer.is_some() {
        ui.draw_pointer(pointer_x, pointer_y);
    }
    let mut left_down = false;
    let mut dragging = false;
    let mut clock_delay = 0usize;
    let mut last_second = 0xffu8;
    loop {
        if clock_delay == 0 {
            clock_delay = 250;
            let runtime = (*system_table).runtime_services;
            if !runtime.is_null() {
                let mut time = Time {
                    year: 0, month: 0, day: 0, hour: 0, minute: 0, second: 0,
                    pad1: 0, nanosecond: 0, time_zone: 0, daylight: 0, pad2: 0,
                };
                if ((*runtime).get_time)(&mut time, null_mut()) == SUCCESS
                    && time.second != last_second
                {
                    last_second = time.second;
                    ui.draw_clock(time.hour, time.minute, time.second);
                }
            }
        } else {
            clock_delay -= 1;
        }

        if let Some(device) = pointer {
            let mut state = SimplePointerState {
                relative_movement_x: 0,
                relative_movement_y: 0,
                relative_movement_z: 0,
                left_button: 0,
                right_button: 0,
            };
            if ((*device).get_state)(device, &mut state) == SUCCESS {
                let dx = pointer_delta(state.relative_movement_x);
                let dy = pointer_delta(state.relative_movement_y);
                pointer_x = ((pointer_x as i64 + dx as i64).max(0) as usize)
                    .min(ui.framebuffer.width.saturating_sub(1));
                pointer_y = ((pointer_y as i64 + dy as i64).max(0) as usize)
                    .min(ui.framebuffer.height.saturating_sub(1));
                let pressed = state.left_button != 0;
                if pressed && !left_down {
                    if ui.title_bar_contains(pointer_x, pointer_y) {
                        dragging = true;
                    } else {
                        let action = ui.click(pointer_x, pointer_y);
                        open_action(action, &mut ui, &mut shell, &installer);
                    }
                }
                if pressed && dragging && (dx != 0 || dy != 0) {
                    ui.move_window(dx, dy);
                }
                if !pressed {
                    dragging = false;
                }
                left_down = pressed;
            }
        }
        if pointer.is_some() {
            ui.draw_pointer(pointer_x, pointer_y);
        }
        if let Some((nic, stack)) = &mut network {
            if let Some(event) = stack.poll(nic) {
                app::show_network_event(&mut ui, event);
            }
        }

        let input = (*system_table).con_in;
        if input.is_null() {
            arch::idle();
            continue;
        }
        let mut key = InputKey { scan_code: 0, unicode_char: 0 };
        let status = ((*input).read_key_stroke)(input, &mut key);
        if status != SUCCESS {
            arch::idle();
            continue;
        }

        let shortcut = match key.scan_code {
            0x000b => UiAction::Launcher, // F1
            0x000c => UiAction::Terminal, // F2
            0x000d => UiAction::Files, // F3
            0x000e => UiAction::Settings, // F4
            0x0012 => UiAction::Installer, // F8
            _ => UiAction::None,
        };
        if shortcut != UiAction::None {
            open_action(shortcut, &mut ui, &mut shell, &installer);
            continue;
        }
        let byte = if key.unicode_char <= 0x7f { key.unicode_char as u8 } else { 0 };
        if byte == 0 {
            continue;
        }
        match shell.feed(byte) {
            FeedResult::Echo(value) => {
                let text = [value];
                ui.write(core::str::from_utf8_unchecked(&text));
            }
            FeedResult::Backspace => ui.write("\x08"),
            FeedResult::Ready => {
                ui.write("\n");
                let line = shell.line();
                if line.eq_ignore_ascii_case("install") {
                    installer.show(&mut ui);
                } else if line.len() > 15
                    && line[..15].eq_ignore_ascii_case("install select ")
                {
                    match line[15..].trim().parse::<usize>() {
                        Ok(number) => installer.select(number, &mut ui),
                        Err(_) => ui.write("Usage: INSTALL SELECT <disk number>\n> "),
                    }
                } else if line.eq_ignore_ascii_case("install erase") {
                    installer.install(system_table, &mut ui);
                } else {
                    app::execute(line, &mut ui, &mut network, "AMD64 UEFI");
                }
                shell.reset();
            }
            FeedResult::Ignored => {}
        }
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    arch::emergency_write("\nBLUEOS UEFI PANIC\n");
    loop { arch::idle(); }
}
