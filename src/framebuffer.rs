use core::ptr::{read_volatile, write_volatile};

use crate::{arch, font};

const BG: u32 = 0x000b_1730;
const BG_LIGHT: u32 = 0x0018_3b68;
const PANEL: u32 = 0x0020_293b;
const PANEL_LIGHT: u32 = 0x002b_3852;
const WINDOW_BORDER: u32 = 0x0047_5a78;
const TERMINAL: u32 = 0x0008_0d18;
const BLUE: u32 = 0x003d_8bfd;
const CYAN: u32 = 0x0048_c7ff;
const WHITE: u32 = 0x00ed_f4ff;
const MUTED: u32 = 0x0098_a9c6;
const GREEN: u32 = 0x0034_d399;
const RED: u32 = 0x00fb_7185;
const MAX_TEXT_COLUMNS: usize = 96;
const MAX_TEXT_ROWS: usize = 36;
const TEXT_CELLS: usize = MAX_TEXT_COLUMNS * MAX_TEXT_ROWS;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum UiAction {
    None,
    Launcher,
    Terminal,
    Files,
    Settings,
    Installer,
}

pub struct Framebuffer {
    address: *mut u8,
    pub width: usize,
    pub height: usize,
    pitch: usize,
    bpp: usize,
    rgb_order: bool,
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
            rgb_order: false,
        }
    }

    /// Create a framebuffer whose bytes are ordered red, green, blue.
    /// Legacy VBE and virtio use `new`, whose memory order is blue first.
    pub const unsafe fn new_rgb(
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
            rgb_order: true,
        }
    }

    pub const fn unavailable() -> Self {
        Self {
            address: core::ptr::null_mut(),
            width: 0,
            height: 0,
            pitch: 0,
            bpp: 0,
            rgb_order: false,
        }
    }

    pub fn available(&self) -> bool {
        !self.address.is_null()
            && (self.bpp == 24 || self.bpp == 32)
            && self.width > 160
            && self.height > 120
    }

    #[inline]
    fn color(&self, x: usize, y: usize) -> u32 {
        if !self.available() || x >= self.width || y >= self.height {
            return 0;
        }
        unsafe {
            let pixel = self.address.add(y * self.pitch + x * (self.bpp / 8));
            if self.rgb_order {
                let red = read_volatile(pixel) as u32;
                let green = read_volatile(pixel.add(1)) as u32;
                let blue = read_volatile(pixel.add(2)) as u32;
                blue | (green << 8) | (red << 16)
            } else if self.bpp == 32 {
                read_volatile(pixel as *const u32) & 0x00ff_ffff
            } else {
                let blue = read_volatile(pixel) as u32;
                let green = read_volatile(pixel.add(1)) as u32;
                let red = read_volatile(pixel.add(2)) as u32;
                blue | (green << 8) | (red << 16)
            }
        }
    }

    #[inline]
    pub fn pixel(&mut self, x: usize, y: usize, color: u32) {
        if !self.available() || x >= self.width || y >= self.height {
            return;
        }
        unsafe {
            let pixel = self.address.add(y * self.pitch + x * (self.bpp / 8));
            let blue = color as u8;
            let green = (color >> 8) as u8;
            let red = (color >> 16) as u8;
            if self.rgb_order {
                write_volatile(pixel, red);
                write_volatile(pixel.add(1), green);
                write_volatile(pixel.add(2), blue);
            } else if self.bpp == 32 {
                write_volatile(pixel as *mut u32, color);
            } else {
                // VBE mode 0x118 is packed BGR888: one byte per channel.
                write_volatile(pixel, blue);
                write_volatile(pixel.add(1), green);
                write_volatile(pixel.add(2), red);
            }
            if self.bpp == 32 && self.rgb_order {
                write_volatile(pixel.add(3), 0);
            }
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
    window_x: usize,
    window_y: usize,
    window_width: usize,
    window_height: usize,
    architecture: [u8; 20],
    architecture_length: usize,
    title: [u8; 32],
    title_length: usize,
    cells: [u8; TEXT_CELLS],
    pointer_saved: [u32; 96],
    pointer_x: usize,
    pointer_y: usize,
    pointer_visible: bool,
    dirty: bool,
}

impl Ui {
    pub fn new(framebuffer: Framebuffer, architecture: &str) -> Self {
        let mut ui = Self {
            framebuffer,
            cursor_column: 0,
            cursor_row: 0,
            terminal_x: 0,
            terminal_y: 0,
            columns: 1,
            rows: 1,
            window_x: 0,
            window_y: 0,
            window_width: 0,
            window_height: 0,
            architecture: [0; 20],
            architecture_length: 0,
            title: [0; 32],
            title_length: 0,
            cells: [0; TEXT_CELLS],
            pointer_saved: [0; 96],
            pointer_x: 0,
            pointer_y: 0,
            pointer_visible: false,
            dirty: true,
        };
        ui.architecture_length = copy_ascii(architecture, &mut ui.architecture);
        ui.draw_desktop();
        ui.open_window("KONSOLE");
        ui
    }

    fn draw_desktop(&mut self) {
        if !self.framebuffer.available() {
            return;
        }
        let width = self.framebuffer.width;
        let height = self.framebuffer.height;

        // Plasma-inspired blue wallpaper. Drawing in horizontal bands keeps
        // boot time small even on firmware framebuffers without acceleration.
        for y in 0..height {
            let shade = (y * 48 / height.max(1)) as u32;
            let color = BG_LIGHT.saturating_sub((shade << 16) | (shade << 8));
            self.framebuffer.rect(0, y, width, 1, color.max(BG));
        }
        self.framebuffer.rect(0, 0, width, 34, 0x0010_1b31);
        self.framebuffer.label(18, 10, "BLUEOS WORKSPACE", WHITE, 2);
        let architecture_bytes = self.architecture;
        let architecture = unsafe {
            core::str::from_utf8_unchecked(&architecture_bytes[..self.architecture_length])
        };
        self.framebuffer.label(width.saturating_sub(176), 12, architecture, MUTED, 1);

        // Desktop application tiles.
        self.desktop_icon(22, 76, BLUE, "HOME");
        self.desktop_icon(22, 164, CYAN, "FILES");
        self.desktop_icon(22, 252, GREEN, "SETTINGS");
        self.desktop_icon(22, 340, RED, "INSTALL");

        // Bottom panel, launcher and a small system tray.
        let panel_y = height.saturating_sub(54);
        self.framebuffer.rect(0, panel_y, width, 54, PANEL);
        self.framebuffer.rect(0, panel_y, width, 2, WINDOW_BORDER);
        self.framebuffer.rect(10, panel_y + 8, 46, 38, BLUE);
        self.framebuffer.label(22, panel_y + 19, "B", WHITE, 2);
        self.framebuffer.rect(66, panel_y + 8, 142, 38, PANEL_LIGHT);
        self.framebuffer.label(80, panel_y + 20, "KONSOLE", WHITE, 1);
        self.framebuffer.rect(width.saturating_sub(238), panel_y + 8, 228, 38, PANEL_LIGHT);
        self.framebuffer.rect(width.saturating_sub(222), panel_y + 20, 10, 10, GREEN);
        self.framebuffer.label(width.saturating_sub(202), panel_y + 20, "AMD64", MUTED, 1);
        self.framebuffer.label(width.saturating_sub(112), panel_y + 20, "--:--:--", WHITE, 1);
    }

    fn desktop_icon(&mut self, x: usize, y: usize, color: u32, text: &str) {
        self.framebuffer.rect(x + 8, y, 46, 46, 0x0007_1020);
        self.framebuffer.rect(x + 12, y + 4, 38, 38, color);
        self.framebuffer.rect(x + 19, y + 11, 24, 24, PANEL);
        self.framebuffer.label(x, y + 54, text, WHITE, 1);
    }

    pub fn open_window(&mut self, title: &str) {
        self.erase_pointer();
        self.title_length = copy_ascii(title, &mut self.title);
        self.cells.fill(0);
        self.cursor_column = 0;
        self.cursor_row = 0;
        if !self.framebuffer.available() {
            return;
        }
        let width = self.framebuffer.width;
        let height = self.framebuffer.height;
        self.window_width = (width * 3 / 4).max(240).min(width.saturating_sub(24));
        self.window_height = (height * 3 / 4)
            .max(180)
            .min(height.saturating_sub(112));
        self.window_x = ((width.saturating_sub(self.window_width)) / 2).max(8);
        self.window_y = 48;
        self.update_terminal_geometry();
        self.draw_window_frame();
        self.dirty = true;
    }

    fn update_terminal_geometry(&mut self) {
        self.terminal_x = self.window_x + 18;
        self.terminal_y = self.window_y + 52;
        self.columns = (self.window_width.saturating_sub(36).max(12) / 12)
            .min(MAX_TEXT_COLUMNS)
            .max(1);
        self.rows = (self.window_height.saturating_sub(70).max(18) / 18)
            .min(MAX_TEXT_ROWS)
            .max(1);
    }

    fn draw_window_frame(&mut self) {
        if !self.framebuffer.available() {
            return;
        }
        let x = self.window_x;
        let y = self.window_y;
        let width = self.window_width;
        let height = self.window_height;
        self.framebuffer.rect(x + 8, y + 8, width, height, 0x0004_0912);
        self.framebuffer.rect(x, y, width, height, WINDOW_BORDER);
        self.framebuffer.rect(x + 2, y + 2, width - 4, height - 4, PANEL);
        self.framebuffer.rect(x + 2, y + 36, width - 4, height - 38, TERMINAL);
        self.framebuffer.rect(x + 14, y + 13, 11, 11, RED);
        self.framebuffer.rect(x + 32, y + 13, 11, 11, 0x00fb_c15b);
        self.framebuffer.rect(x + 50, y + 13, 11, 11, GREEN);
        let title_bytes = self.title;
        let title = unsafe { core::str::from_utf8_unchecked(&title_bytes[..self.title_length]) };
        self.framebuffer.label(x + 78, y + 14, title, WHITE, 1);
    }

    pub fn open_launcher(&mut self) {
        self.open_window("APPLICATION LAUNCHER");
        self.write("BLUEOS APPLICATIONS\n\n");
        self.write("[1] TERMINAL      [2] FILES\n");
        self.write("[3] SETTINGS      [4] LUA STUDIO\n");
        self.write("[5] DISK INSTALLER\n\n");
        self.write("Commands: DESKTOP FILES SETTINGS INSTALL LUA\n> ");
    }

    pub fn open_files(&mut self) {
        self.open_window("DOLPHIN-LIKE FILE MANAGER");
        self.write("PLACES              NAME             TYPE\n");
        self.write("HOME                APPS             DIRECTORY\n");
        self.write("SYSTEM              CONFIG           DIRECTORY\n");
        self.write("DEVICES             README.TXT       DOCUMENT\n\n");
        self.write("BlueOS currently exposes a read-only virtual system tree.\n> ");
    }

    pub fn open_settings(&mut self) {
        self.open_window("SYSTEM SETTINGS");
        self.write("WORKSPACE THEME      BLUE PLASMA\n");
        self.write("DISPLAY              FRAMEBUFFER ONLINE\n");
        self.write("INPUT                FIRMWARE / PS2\n");
        self.write("PLATFORM             AMD64 UEFI + BIOS\n");
        self.write("NETWORK              E1000 / VIRTIO / UEFI SNP\n\n> ");
    }

    pub fn open_installer(&mut self) {
        self.open_window("BLUEOS DISK INSTALLER");
        self.write("INSTALLATION MODE\n\n");
        self.write("The live USB image can be cloned to a selected disk.\n");
        self.write("All data on the destination will be destroyed.\n\n");
    }

    pub fn title_bar_contains(&self, x: usize, y: usize) -> bool {
        x >= self.window_x
            && x < self.window_x.saturating_add(self.window_width)
            && y >= self.window_y
            && y < self.window_y.saturating_add(36)
    }

    pub fn move_window(&mut self, dx: i32, dy: i32) {
        if !self.framebuffer.available() {
            return;
        }
        let max_x = self.framebuffer.width.saturating_sub(self.window_width + 8).max(8);
        let max_y = self.framebuffer.height
            .saturating_sub(54 + self.window_height + 8)
            .max(36);
        self.window_x = ((self.window_x as i64 + dx as i64).max(8) as usize).min(max_x);
        self.window_y = ((self.window_y as i64 + dy as i64).max(36) as usize).min(max_y);
        self.update_terminal_geometry();
        self.redraw();
    }

    pub fn click(&self, x: usize, y: usize) -> UiAction {
        if !self.framebuffer.available() {
            return UiAction::None;
        }
        let panel_y = self.framebuffer.height.saturating_sub(54);
        if y >= panel_y {
            if x < 62 {
                return UiAction::Launcher;
            }
            if (66..=208).contains(&x) {
                return UiAction::Terminal;
            }
        }
        if x <= 92 {
            if (70..=145).contains(&y) {
                return UiAction::Launcher;
            }
            if (158..=233).contains(&y) {
                return UiAction::Files;
            }
            if (246..=321).contains(&y) {
                return UiAction::Settings;
            }
            if (334..=420).contains(&y) {
                return UiAction::Installer;
            }
        }
        UiAction::None
    }

    pub fn draw_clock(&mut self, hour: u8, minute: u8, second: u8) {
        self.erase_pointer();
        if !self.framebuffer.available() {
            return;
        }
        let width = self.framebuffer.width;
        let panel_y = self.framebuffer.height.saturating_sub(54);
        let mut clock = *b"00:00:00";
        clock[0] = b'0' + (hour / 10) % 10;
        clock[1] = b'0' + hour % 10;
        clock[3] = b'0' + (minute / 10) % 10;
        clock[4] = b'0' + minute % 10;
        clock[6] = b'0' + (second / 10) % 10;
        clock[7] = b'0' + second % 10;
        self.framebuffer.rect(width.saturating_sub(116), panel_y + 14, 102, 22, PANEL_LIGHT);
        let text = unsafe { core::str::from_utf8_unchecked(&clock) };
        self.framebuffer.label(width.saturating_sub(112), panel_y + 20, text, WHITE, 1);
    }

    pub fn draw_pointer(&mut self, x: usize, y: usize) {
        self.erase_pointer();
        if !self.framebuffer.available() {
            return;
        }
        self.pointer_x = x.min(self.framebuffer.width.saturating_sub(1));
        self.pointer_y = y.min(self.framebuffer.height.saturating_sub(1));
        const SHAPE: [u8; 12] = [
            0b1000_0000, 0b1100_0000, 0b1110_0000, 0b1111_0000,
            0b1111_1000, 0b1111_1100, 0b1110_0000, 0b1011_0000,
            0b0011_0000, 0b0001_1000, 0b0001_1000, 0b0000_0000,
        ];
        for row in 0..12 {
            for column in 0..8 {
                let px = self.pointer_x + column;
                let py = self.pointer_y + row;
                let index = row * 8 + column;
                self.pointer_saved[index] = self.framebuffer.color(px, py);
                if SHAPE[row] & (0x80 >> column) != 0 {
                    let edge = column == 0 || row == 0 || SHAPE[row] & (0x40 >> column) == 0;
                    self.framebuffer.pixel(px, py, if edge { TERMINAL } else { WHITE });
                }
            }
        }
        self.pointer_visible = true;
    }

    fn erase_pointer(&mut self) {
        if !self.pointer_visible {
            return;
        }
        for row in 0..12 {
            for column in 0..8 {
                self.framebuffer.pixel(
                    self.pointer_x + column,
                    self.pointer_y + row,
                    self.pointer_saved[row * 8 + column],
                );
            }
        }
        self.pointer_visible = false;
    }

    fn redraw(&mut self) {
        self.erase_pointer();
        if !self.framebuffer.available() {
            return;
        }
        self.draw_desktop();
        self.draw_window_frame();
        for row in 0..self.rows {
            for column in 0..self.columns {
                let byte = self.cells[row * MAX_TEXT_COLUMNS + column];
                if byte != 0 {
                    self.framebuffer.character(
                        self.terminal_x + column * 12,
                        self.terminal_y + row * 18,
                        byte,
                        WHITE,
                        2,
                    );
                }
            }
        }
        self.dirty = true;
    }

    fn clear_terminal_area(&mut self) {
        self.cells.fill(0);
        if self.framebuffer.available() {
            self.framebuffer.rect(
                self.terminal_x,
                self.terminal_y.saturating_sub(4),
                self.columns.saturating_mul(12),
                self.rows.saturating_mul(18).saturating_add(4),
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
        self.erase_pointer();
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
                if self.cursor_row < self.rows {
                    self.cells[self.cursor_row * MAX_TEXT_COLUMNS + self.cursor_column] = 0;
                }
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
        if self.cursor_row < self.rows && self.cursor_column < self.columns {
            self.cells[self.cursor_row * MAX_TEXT_COLUMNS + self.cursor_column] = byte;
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

fn copy_ascii(source: &str, destination: &mut [u8]) -> usize {
    let length = source.len().min(destination.len());
    destination[..length].copy_from_slice(&source.as_bytes()[..length]);
    length
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
