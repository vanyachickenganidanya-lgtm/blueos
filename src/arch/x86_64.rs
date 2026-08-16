use core::{arch::asm, ptr::{read_volatile, write_volatile}};

use crate::{
    app,
    framebuffer::{Framebuffer, Ui, UiAction},
    net::{NetworkStack, Nic},
    shell::{FeedResult, Shell},
};

const BOOT_MAGIC: u64 = 0x4555_4c42_534f_554c;

#[repr(C)]
pub struct BootInfo {
    magic: u64,
    framebuffer: u64,
    width: u32,
    height: u32,
    pitch: u32,
    bits_per_pixel: u32,
    boot_drive: u8,
    _padding: [u8; 7],
}

extern "C" {
    static mut __bss_start: u8;
    static mut __bss_end: u8;
}

#[no_mangle]
#[link_section = ".text.boot"]
pub unsafe extern "C" fn _start(boot_info: *const BootInfo) -> ! {
    clear_bss();
    serial_init();
    debug_write("\nBlueOS x86_64 kernel entry\n");

    let info = &*boot_info;
    let framebuffer = if info.magic == BOOT_MAGIC
        && info.framebuffer != 0
        && (info.bits_per_pixel == 24 || info.bits_per_pixel == 32)
    {
        debug_write("VBE framebuffer ready\n");
        Framebuffer::new(
            info.framebuffer as usize,
            info.width as usize,
            info.height as usize,
            info.pitch as usize,
            info.bits_per_pixel as usize,
        )
    } else {
        debug_write("VBE framebuffer unavailable; serial console only\n");
        Framebuffer::unavailable()
    };

    let mut ui = Ui::new(framebuffer, "X86_64");
    let mut network = E1000::probe().map(|nic| {
        let stack = NetworkStack::new(nic.mac_address());
        (nic, stack)
    });
    if let Some((nic, stack)) = &mut network {
        stack.start(nic);
    }
    app::banner(&mut ui, "x86_64", network.is_some());
    app::run_lua_demo(&mut ui);
    ui.write("> ");

    let mut keyboard = Keyboard::new();
    let mut mouse = Mouse::new(ui.framebuffer.width, ui.framebuffer.height);
    if mouse.enabled {
        ui.draw_pointer(mouse.x, mouse.y);
    }
    let mut shell = Shell::new();
    let mut dragging = false;
    let mut clock_delay = 0usize;
    let mut last_second = 0xffu8;
    loop {
        if clock_delay == 0 {
            clock_delay = 100_000;
            if let Some((hour, minute, second)) = rtc_time() {
                if second != last_second {
                    last_second = second;
                    ui.draw_clock(hour, minute, second);
                }
            }
        } else {
            clock_delay -= 1;
        }
        if let Some(byte) = keyboard.poll() {
            let action = match byte {
                0xf1 => UiAction::Launcher,
                0xf2 => UiAction::Terminal,
                0xf3 => UiAction::Files,
                0xf4 => UiAction::Settings,
                0xf8 => UiAction::Installer,
                _ => UiAction::None,
            };
            if action != UiAction::None {
                open_ui_action(action, &mut ui, &mut shell);
                continue;
            }
            match shell.feed(byte) {
                FeedResult::Echo(byte) => {
                    let text = [byte];
                    ui.write(unsafe { core::str::from_utf8_unchecked(&text) });
                }
                FeedResult::Backspace => ui.write("\x08"),
                FeedResult::Ready => {
                    ui.write("\n");
                    app::execute(shell.line(), &mut ui, &mut network, "x86_64");
                    shell.reset();
                }
                FeedResult::Ignored => {}
            }
        }
        if let Some(event) = mouse.poll() {
            if event.left && !event.was_left {
                if ui.title_bar_contains(event.x, event.y) {
                    dragging = true;
                } else {
                    open_ui_action(ui.click(event.x, event.y), &mut ui, &mut shell);
                }
            }
            if event.left && dragging && (event.dx != 0 || event.dy != 0) {
                ui.move_window(event.dx, event.dy);
            }
            if !event.left {
                dragging = false;
            }
        }
        if let Some((nic, stack)) = &mut network {
            if let Some(event) = stack.poll(nic) {
                app::show_network_event(&mut ui, event);
            }
        }
        if mouse.enabled {
            ui.draw_pointer(mouse.x, mouse.y);
        }
        idle();
    }
}

unsafe fn clear_bss() {
    let mut cursor = core::ptr::addr_of_mut!(__bss_start);
    let end = core::ptr::addr_of_mut!(__bss_end);
    while cursor < end {
        write_volatile(cursor, 0);
        cursor = cursor.add(1);
    }
}

pub fn idle() {
    unsafe { asm!("pause", options(nomem, nostack, preserves_flags)) }
}

const COM1: u16 = 0x3f8;

unsafe fn serial_init() {
    out8(COM1 + 1, 0x00);
    out8(COM1 + 3, 0x80);
    out8(COM1, 0x03);
    out8(COM1 + 1, 0x00);
    out8(COM1 + 3, 0x03);
    out8(COM1 + 2, 0xc7);
    out8(COM1 + 4, 0x0b);
}

pub fn debug_write(text: &str) {
    for byte in text.bytes() {
        if byte == b'\n' {
            serial_byte(b'\r');
        }
        serial_byte(byte);
    }
}

pub fn emergency_write(text: &str) {
    debug_write(text);
}

fn serial_byte(byte: u8) {
    for _ in 0..100_000 {
        if unsafe { in8(COM1 + 5) } & 0x20 != 0 {
            unsafe { out8(COM1, byte) };
            return;
        }
    }
}

#[inline]
unsafe fn in8(port: u16) -> u8 {
    let value: u8;
    asm!("in al, dx", in("dx") port, out("al") value, options(nomem, nostack, preserves_flags));
    value
}

#[inline]
unsafe fn out8(port: u16, value: u8) {
    asm!("out dx, al", in("dx") port, in("al") value, options(nomem, nostack, preserves_flags));
}

#[inline]
unsafe fn in32(port: u16) -> u32 {
    let value: u32;
    asm!("in eax, dx", in("dx") port, out("eax") value, options(nomem, nostack, preserves_flags));
    value
}

#[inline]
unsafe fn out32(port: u16, value: u32) {
    asm!("out dx, eax", in("dx") port, in("eax") value, options(nomem, nostack, preserves_flags));
}

struct Keyboard {
    shift: bool,
}

impl Keyboard {
    const fn new() -> Self {
        Self { shift: false }
    }

    fn poll(&mut self) -> Option<u8> {
        let status = unsafe { in8(0x64) };
        if status & 1 == 0 || status & 0x20 != 0 {
            return None;
        }
        let scan = unsafe { in8(0x60) };
        match scan {
            0x2a | 0x36 => {
                self.shift = true;
                None
            }
            0xaa | 0xb6 => {
                self.shift = false;
                None
            }
            value if value & 0x80 != 0 => None,
            value => scan_code(value, self.shift),
        }
    }
}

fn scan_code(code: u8, shift: bool) -> Option<u8> {
    let normal = match code {
        0x3b => 0xf1,
        0x3c => 0xf2,
        0x3d => 0xf3,
        0x3e => 0xf4,
        0x42 => 0xf8,
        0x02..=0x0b => b"1234567890"[(code - 0x02) as usize],
        0x10..=0x19 => b"qwertyuiop"[(code - 0x10) as usize],
        0x1e..=0x26 => b"asdfghjkl"[(code - 0x1e) as usize],
        0x2c..=0x32 => b"zxcvbnm"[(code - 0x2c) as usize],
        0x0c => b'-',
        0x0d => b'=',
        0x1a => b'[',
        0x1b => b']',
        0x27 => b';',
        0x28 => b'\'',
        0x29 => b'`',
        0x2b => b'\\',
        0x33 => b',',
        0x34 => b'.',
        0x35 => b'/',
        0x39 => b' ',
        0x1c => b'\n',
        0x0e => 8,
        _ => return None,
    };
    if !shift {
        return Some(normal);
    }
    Some(match normal {
        b'a'..=b'z' => normal - 32,
        b'1' => b'!', b'2' => b'@', b'3' => b'#', b'4' => b'$', b'5' => b'%',
        b'6' => b'^', b'7' => b'&', b'8' => b'*', b'9' => b'(', b'0' => b')',
        b'-' => b'_', b'=' => b'+', b'[' => b'{', b']' => b'}', b';' => b':',
        b'\'' => b'"', b',' => b'<', b'.' => b'>', b'/' => b'?', b'\\' => b'|',
        other => other,
    })
}

fn open_ui_action(action: UiAction, ui: &mut Ui, shell: &mut Shell) {
    match action {
        UiAction::Launcher => ui.open_launcher(),
        UiAction::Terminal => {
            ui.open_window("KONSOLE");
            ui.write("> ");
        }
        UiAction::Files => ui.open_files(),
        UiAction::Settings => ui.open_settings(),
        UiAction::Installer => {
            ui.open_installer();
            ui.write("Physical disk writes are enabled only in the UEFI live workspace.\n> ");
        }
        UiAction::None => return,
    }
    shell.reset();
}

struct MouseEvent {
    x: usize,
    y: usize,
    dx: i32,
    dy: i32,
    left: bool,
    was_left: bool,
}

struct Mouse {
    packet: [u8; 3],
    packet_index: usize,
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    left: bool,
    enabled: bool,
}

impl Mouse {
    fn new(width: usize, height: usize) -> Self {
        let enabled = unsafe { initialize_ps2_mouse() };
        if enabled {
            debug_write("PS/2 mouse online\n");
        } else {
            debug_write("PS/2 mouse unavailable\n");
        }
        Self {
            packet: [0; 3],
            packet_index: 0,
            x: width / 2,
            y: height / 2,
            width,
            height,
            left: false,
            enabled,
        }
    }

    fn poll(&mut self) -> Option<MouseEvent> {
        if !self.enabled {
            return None;
        }
        let status = unsafe { in8(0x64) };
        if status & 0x21 != 0x21 {
            return None;
        }
        let byte = unsafe { in8(0x60) };
        if self.packet_index == 0 && byte & 0x08 == 0 {
            return None;
        }
        self.packet[self.packet_index] = byte;
        self.packet_index += 1;
        if self.packet_index != 3 {
            return None;
        }
        self.packet_index = 0;
        if self.packet[0] & 0xc0 != 0 {
            return None;
        }
        let dx = self.packet[1] as i8 as i32;
        let dy = -(self.packet[2] as i8 as i32);
        self.x = ((self.x as i64 + dx as i64).max(0) as usize)
            .min(self.width.saturating_sub(1));
        self.y = ((self.y as i64 + dy as i64).max(0) as usize)
            .min(self.height.saturating_sub(1));
        let was_left = self.left;
        self.left = self.packet[0] & 1 != 0;
        Some(MouseEvent {
            x: self.x,
            y: self.y,
            dx,
            dy,
            left: self.left,
            was_left,
        })
    }
}

unsafe fn ps2_wait_write() -> bool {
    for _ in 0..100_000 {
        if in8(0x64) & 2 == 0 {
            return true;
        }
        core::hint::spin_loop();
    }
    false
}

unsafe fn ps2_wait_read() -> bool {
    for _ in 0..100_000 {
        if in8(0x64) & 1 != 0 {
            return true;
        }
        core::hint::spin_loop();
    }
    false
}

unsafe fn ps2_mouse_command(command: u8) -> bool {
    if !ps2_wait_write() {
        return false;
    }
    out8(0x64, 0xd4);
    if !ps2_wait_write() {
        return false;
    }
    out8(0x60, command);
    ps2_wait_read() && in8(0x60) == 0xfa
}

unsafe fn initialize_ps2_mouse() -> bool {
    for _ in 0..32 {
        if in8(0x64) & 1 == 0 {
            break;
        }
        let _ = in8(0x60);
    }
    if !ps2_wait_write() {
        return false;
    }
    out8(0x64, 0xa8);
    if !ps2_wait_write() {
        return false;
    }
    out8(0x64, 0x20);
    if !ps2_wait_read() {
        return false;
    }
    let configuration = (in8(0x60) | 0x02) & !0x20;
    if !ps2_wait_write() {
        return false;
    }
    out8(0x64, 0x60);
    if !ps2_wait_write() {
        return false;
    }
    out8(0x60, configuration);
    ps2_mouse_command(0xf6) && ps2_mouse_command(0xf4)
}

fn rtc_register(register: u8) -> u8 {
    unsafe {
        out8(0x70, register);
        in8(0x71)
    }
}

fn rtc_time() -> Option<(u8, u8, u8)> {
    if rtc_register(0x0a) & 0x80 != 0 {
        return None;
    }
    let first_second = rtc_register(0x00);
    let minute = rtc_register(0x02);
    let raw_hour = rtc_register(0x04);
    let mode = rtc_register(0x0b);
    if first_second != rtc_register(0x00) {
        return None;
    }
    let decode = |value: u8| {
        if mode & 0x04 != 0 { value } else { (value & 0x0f) + ((value >> 4) * 10) }
    };
    let second = decode(first_second);
    let minute = decode(minute);
    let mut hour = decode(raw_hour & 0x7f);
    if mode & 0x02 == 0 {
        let afternoon = raw_hour & 0x80 != 0;
        hour %= 12;
        if afternoon {
            hour += 12;
        }
    }
    Some((hour, minute, second))
}

/* ------------------------------ e1000 ------------------------------ */

const RX_COUNT: usize = 16;
const TX_COUNT: usize = 8;
const BUFFER_SIZE: usize = 2048;

#[repr(C)]
#[derive(Clone, Copy)]
struct RxDescriptor {
    address: u64,
    length: u16,
    checksum: u16,
    status: u8,
    errors: u8,
    special: u16,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct TxDescriptor {
    address: u64,
    length: u16,
    checksum_offset: u8,
    command: u8,
    status: u8,
    checksum_start: u8,
    special: u16,
}

const EMPTY_RX: RxDescriptor = RxDescriptor {
    address: 0,
    length: 0,
    checksum: 0,
    status: 0,
    errors: 0,
    special: 0,
};
const EMPTY_TX: TxDescriptor = TxDescriptor {
    address: 0,
    length: 0,
    checksum_offset: 0,
    command: 0,
    status: 1,
    checksum_start: 0,
    special: 0,
};

#[repr(align(128))]
struct RxRing([RxDescriptor; RX_COUNT]);
#[repr(align(128))]
struct TxRing([TxDescriptor; TX_COUNT]);
#[repr(align(4096))]
struct RxBuffers([[u8; BUFFER_SIZE]; RX_COUNT]);
#[repr(align(4096))]
struct TxBuffers([[u8; BUFFER_SIZE]; TX_COUNT]);

static mut RX_RING: RxRing = RxRing([EMPTY_RX; RX_COUNT]);
static mut TX_RING: TxRing = TxRing([EMPTY_TX; TX_COUNT]);
static mut RX_BUFFERS: RxBuffers = RxBuffers([[0; BUFFER_SIZE]; RX_COUNT]);
static mut TX_BUFFERS: TxBuffers = TxBuffers([[0; BUFFER_SIZE]; TX_COUNT]);

pub struct E1000 {
    mmio: *mut u32,
    mac: [u8; 6],
    receive_index: usize,
    transmit_index: usize,
}

impl E1000 {
    fn probe() -> Option<Self> {
        for device in 0..32u8 {
            let id = pci_read(0, device, 0, 0);
            if id == 0xffff_ffff {
                continue;
            }
            let vendor = id as u16;
            let class = pci_read(0, device, 0, 8) >> 24;
            if vendor == 0x8086 && class == 0x02 {
                let bar = pci_read(0, device, 0, 0x10);
                if bar & 1 != 0 {
                    continue;
                }
                let mut command = pci_read(0, device, 0, 4);
                command |= (1 << 1) | (1 << 2);
                pci_write(0, device, 0, 4, command);
                let mmio = (bar & 0xffff_fff0) as usize as *mut u32;
                return unsafe { Self::initialize(mmio) };
            }
        }
        None
    }

    unsafe fn initialize(mmio: *mut u32) -> Option<Self> {
        let register = |offset: usize| mmio.add(offset / 4);
        write_volatile(register(0x0000), read_volatile(register(0x0000)) | (1 << 26));
        for _ in 0..200_000 {
            core::hint::spin_loop();
        }
        write_volatile(register(0x00d8), 0xffff_ffff); // IMC
        write_volatile(register(0x0000), read_volatile(register(0x0000)) | (1 << 6)); // link up

        let ral = read_volatile(register(0x5400));
        let rah = read_volatile(register(0x5404));
        let mac = [
            ral as u8,
            (ral >> 8) as u8,
            (ral >> 16) as u8,
            (ral >> 24) as u8,
            rah as u8,
            (rah >> 8) as u8,
        ];
        if mac == [0; 6] {
            return None;
        }

        for index in 0..RX_COUNT {
            let descriptor = core::ptr::addr_of_mut!(RX_RING.0[index]);
            write_volatile(descriptor, EMPTY_RX);
            (*descriptor).address = core::ptr::addr_of_mut!(RX_BUFFERS.0[index]) as u64;
        }
        let rx_address = core::ptr::addr_of!(RX_RING.0) as u64;
        write_volatile(register(0x2800), rx_address as u32);
        write_volatile(register(0x2804), (rx_address >> 32) as u32);
        write_volatile(register(0x2808), (RX_COUNT * core::mem::size_of::<RxDescriptor>()) as u32);
        write_volatile(register(0x2810), 0);
        write_volatile(register(0x2818), (RX_COUNT - 1) as u32);
        write_volatile(register(0x0100), (1 << 1) | (1 << 15) | (1 << 26));

        for index in 0..TX_COUNT {
            let descriptor = core::ptr::addr_of_mut!(TX_RING.0[index]);
            write_volatile(descriptor, EMPTY_TX);
            (*descriptor).address = core::ptr::addr_of_mut!(TX_BUFFERS.0[index]) as u64;
        }
        let tx_address = core::ptr::addr_of!(TX_RING.0) as u64;
        write_volatile(register(0x3800), tx_address as u32);
        write_volatile(register(0x3804), (tx_address >> 32) as u32);
        write_volatile(register(0x3808), (TX_COUNT * core::mem::size_of::<TxDescriptor>()) as u32);
        write_volatile(register(0x3810), 0);
        write_volatile(register(0x3818), 0);
        write_volatile(register(0x0410), 10 | (8 << 10) | (6 << 20));
        write_volatile(register(0x0400), (1 << 1) | (1 << 3) | (15 << 4) | (64 << 12));

        Some(Self {
            mmio,
            mac,
            receive_index: 0,
            transmit_index: 0,
        })
    }

    fn write_register(&self, offset: usize, value: u32) {
        unsafe { write_volatile(self.mmio.add(offset / 4), value) }
    }
}

impl Nic for E1000 {
    fn mac_address(&self) -> [u8; 6] {
        self.mac
    }

    fn receive(&mut self, packet: &mut [u8]) -> Option<usize> {
        unsafe {
            let descriptor = core::ptr::addr_of_mut!(RX_RING.0[self.receive_index]);
            if read_volatile(core::ptr::addr_of!((*descriptor).status)) & 1 == 0 {
                return None;
            }
            let length = (read_volatile(core::ptr::addr_of!((*descriptor).length)) as usize)
                .min(packet.len())
                .min(BUFFER_SIZE);
            let source = core::ptr::addr_of!(RX_BUFFERS.0[self.receive_index]) as *const u8;
            for (index, output) in packet[..length].iter_mut().enumerate() {
                *output = read_volatile(source.add(index));
            }
            write_volatile(core::ptr::addr_of_mut!((*descriptor).status), 0);
            self.write_register(0x2818, self.receive_index as u32);
            self.receive_index = (self.receive_index + 1) % RX_COUNT;
            Some(length)
        }
    }

    fn transmit(&mut self, packet: &[u8]) -> bool {
        if packet.len() > BUFFER_SIZE {
            return false;
        }
        unsafe {
            let descriptor = core::ptr::addr_of_mut!(TX_RING.0[self.transmit_index]);
            if read_volatile(core::ptr::addr_of!((*descriptor).status)) & 1 == 0 {
                return false;
            }
            let destination = core::ptr::addr_of_mut!(TX_BUFFERS.0[self.transmit_index]) as *mut u8;
            for (index, byte) in packet.iter().enumerate() {
                write_volatile(destination.add(index), *byte);
            }
            write_volatile(core::ptr::addr_of_mut!((*descriptor).length), packet.len() as u16);
            write_volatile(core::ptr::addr_of_mut!((*descriptor).command), 0x0b); // EOP, IFCS, RS
            write_volatile(core::ptr::addr_of_mut!((*descriptor).status), 0);
            core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
            self.transmit_index = (self.transmit_index + 1) % TX_COUNT;
            self.write_register(0x3818, self.transmit_index as u32);
            true
        }
    }
}

fn pci_read(bus: u8, device: u8, function: u8, offset: u8) -> u32 {
    let address = 0x8000_0000
        | ((bus as u32) << 16)
        | ((device as u32) << 11)
        | ((function as u32) << 8)
        | ((offset as u32) & 0xfc);
    unsafe {
        out32(0xcf8, address);
        in32(0xcfc)
    }
}

fn pci_write(bus: u8, device: u8, function: u8, offset: u8, value: u32) {
    let address = 0x8000_0000
        | ((bus as u32) << 16)
        | ((device as u32) << 11)
        | ((function as u32) << 8)
        | ((offset as u32) & 0xfc);
    unsafe {
        out32(0xcf8, address);
        out32(0xcfc, value);
    }
}
