use core::ptr::write_volatile;

use crate::{arch, font};

const BG: u32 = 0x0010_172a;
const PANEL: u32 = 0x0018_2238;
const TERMINAL: u32 = 0x0008_0d18;
const BLUE: u32 = 0x002f_81f7;
const CYAN: u32 = 0x0038_bdf8;
const WHITE: u32 = 0x00e6_edff;
const MUTED: u32 = 0x008a_9ab8;
const GREEN: u32 = 0x0034_d399;

pub struct Framebuffer {
    address: *mut u8,
    pub width: usize,
    pub height: usize,
    pitch: usize,
    bpp: usize,
}

impl Framebuffer {
    pub const unsafe fn new(
        address: usize,
        width: usize,
        height: usize,
        pitch: usize,
        bpp: usize,
    ) -> Self {
        Self {
            address: address as *mut u8,
            width,
            height,
            pitch,
            bpp,
        }
    }

    pub const fn unavailable() -> Self {
        Self {
            address: core::ptr::null_mut(),
            width: 0,
            height: 0,
            pitch: 0,
            bpp: 0,
        }
    }

    pub fn available(&self) -> bool {
        !self.address.is_null() && self.bpp == 32 && self.width > 160 && self.height > 120
    }

    #[inline]
    pub fn pixel(&mut self, x: usize, y: usize, color: u32) {
        if !self.available() || x >= self.width || y >= self.height {
            return;
        }
        unsafe {
            write_volatile(self.address.add(y * self.pitch + x * 4) as *mut u32, color);
        }
    }

    pub fn rect(&mut self, x: usize, y: usize, width: usize, height: usize, color: u32) {
        let max_x = x.saturating_add(width).min(self.width);
        let max_y = y.saturating_add(height).min(self.height);
        for py in y..max_y {
            for px in x..max_x {
                self.pixel(px, py, color);
            }
        }
    }

    pub fn character(&mut self, x: usize, y: usize, byte: u8, color: u32, scale: usize) {
        let rows = font::glyph(byte);
        for (row, bits) in rows.iter().enumerate() {
            for column in 0..5 {
                if bits & (1 << (4 - column)) != 0 {
                    self.rect(x + column * scale, y + row * scale, scale, scale, color);
                }
            }
        }
    }

    pub fn label(&mut self, mut x: usize, y: usize, text: &str, color: u32, scale: usize) {
        for byte in text.bytes() {
            self.character(x, y, byte, color, scale);
            x += 6 * scale;
        }
    }
}

pub struct Ui {
    pub framebuffer: Framebuffer,
    cursor_column: usize,
    cursor_row: usize,
    terminal_x: usize,
    terminal_y: usize,
    columns: usize,
    rows: usize,
    dirty: bool,
}

impl Ui {
    pub fn new(framebuffer: Framebuffer, architecture: &str) -> Self {
        let available = framebuffer.available();
        let width = framebuffer.width;
        let height = framebuffer.height;
        let terminal_x = if available { 30 } else { 0 };
        let terminal_y = if available { 142 } else { 0 };
        let columns = width.saturating_sub(60) / 12;
        let rows = height.saturating_sub(174) / 18;
        let mut ui = Self {
            framebuffer,
            cursor_column: 0,
            cursor_row: 0,
            terminal_x,
            terminal_y,
            columns: columns.max(1),
            rows: rows.max(1),
            dirty: true,
        };
        ui.draw_desktop(architecture);
        ui
    }

    fn draw_desktop(&mut self, architecture: &str) {
        if !self.framebuffer.available() {
            return;
        }
        let width = self.framebuffer.width;
        let height = self.framebuffer.height;
        self.framebuffer.rect(0, 0, width, height, BG);
        self.framebuffer.rect(0, 0, width, 64, PANEL);
        self.framebuffer.rect(0, 63, width, 2, BLUE);

        // A small, deliberately simple graphical BlueOS mark.
        self.framebuffer.rect(26, 18, 30, 30, BLUE);
        self.framebuffer.rect(32, 12, 18, 42, CYAN);
        self.framebuffer.label(74, 21, "BLUEOS", WHITE, 3);
        self.framebuffer.label(width.saturating_sub(176), 25, architecture, MUTED, 2);

        self.framebuffer.rect(24, 82, 212, 42, PANEL);
        self.framebuffer.label(38, 94, "GRAPHICS", MUTED, 1);
        self.framebuffer.label(124, 91, "ONLINE", GREEN, 2);
        self.framebuffer.rect(248, 82, 212, 42, PANEL);
        self.framebuffer.label(262, 94, "LUA VM", MUTED, 1);
        self.framebuffer.label(332, 91, "READY", GREEN, 2);
        self.framebuffer.rect(472, 82, 212, 42, PANEL);
        self.framebuffer.label(486, 94, "NETWORK", MUTED, 1);
        self.framebuffer.label(568, 91, "PROBE", CYAN, 2);

        self.clear_terminal_area();
    }

    fn clear_terminal_area(&mut self) {
        if self.framebuffer.available() {
            self.framebuffer.rect(
                self.terminal_x.saturating_sub(12),
                self.terminal_y.saturating_sub(10),
                self.framebuffer.width.saturating_sub(self.terminal_x * 2).saturating_add(24),
                self.framebuffer.height.saturating_sub(self.terminal_y).saturating_sub(18),
                TERMINAL,
            );
        }
        self.cursor_column = 0;
        self.cursor_row = 0;
        self.dirty = true;
    }

    pub fn clear(&mut self) {
        self.clear_terminal_area();
        self.write("BlueOS console cleared\n> ");
    }

    pub fn write(&mut self, text: &str) {
        arch::debug_write(text);
        for byte in text.bytes() {
            self.put_byte(byte);
        }
    }

    pub fn write_number(&mut self, number: i64) {
        let mut bytes = [0u8; 21];
        let text = format_i64(number, &mut bytes);
        self.write(text);
    }

    pub fn write_ipv4(&mut self, address: [u8; 4]) {
        let mut buffer = [0u8; 3];
        for (index, octet) in address.iter().enumerate() {
            let text = format_u64(*octet as u64, &mut buffer);
            self.write(text);
            if index != 3 {
                self.write(".");
            }
        }
    }

    fn put_byte(&mut self, byte: u8) {
        if byte == b'\n' {
            self.newline();
            return;
        }
        if byte == b'\r' {
            self.cursor_column = 0;
            return;
        }
        if byte == 8 || byte == 127 {
            if self.cursor_column > 0 {
                self.cursor_column -= 1;
                let x = self.terminal_x + self.cursor_column * 12;
                let y = self.terminal_y + self.cursor_row * 18;
                self.framebuffer.rect(x, y, 12, 16, TERMINAL);
                self.dirty = true;
            }
            return;
        }
        if self.cursor_column >= self.columns {
            self.newline();
        }
        if self.framebuffer.available() {
            let x = self.terminal_x + self.cursor_column * 12;
            let y = self.terminal_y + self.cursor_row * 18;
            self.framebuffer.character(x, y, byte, WHITE, 2);
        }
        self.cursor_column += 1;
        self.dirty = true;
    }

    fn newline(&mut self) {
        self.cursor_column = 0;
        self.cursor_row += 1;
        if self.cursor_row >= self.rows {
            self.clear_terminal_area();
        }
    }

    pub fn take_dirty(&mut self) -> bool {
        let was_dirty = self.dirty;
        self.dirty = false;
        was_dirty
    }
}

fn format_i64<'a>(number: i64, output: &'a mut [u8; 21]) -> &'a str {
    if number < 0 {
        output[0] = b'-';
        let unsigned = number.wrapping_neg() as u64;
        let length = format_u64(unsigned, &mut output[1..]).len();
        unsafe { core::str::from_utf8_unchecked(&output[..length + 1]) }
    } else {
        format_u64(number as u64, output)
    }
}

fn format_u64<'a>(mut number: u64, output: &'a mut [u8]) -> &'a str {
    let mut scratch = [0u8; 20];
    let mut count = 0;
    loop {
        scratch[count] = b'0' + (number % 10) as u8;
        count += 1;
        number /= 10;
        if number == 0 {
            break;
        }
    }
    for index in 0..count {
        output[index] = scratch[count - index - 1];
    }
    unsafe { core::str::from_utf8_unchecked(&output[..count]) }
}
