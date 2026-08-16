use core::{arch::{asm, global_asm}, ptr::{read_volatile, write_volatile}};

use crate::{
    app,
    framebuffer::{Framebuffer, Ui},
    net::{NetworkStack, Nic},
    shell::{FeedResult, Shell},
};

#[path = "riscv64/virtio.rs"]
mod virtio;
use virtio::{VirtioGpu, VirtioNet};

global_asm!(include_str!("riscv64/boot.S"));

const UART: usize = 0x1000_0000;

#[no_mangle]
pub extern "C" fn riscv_main(hart_id: usize, dtb: usize) -> ! {
    uart_init();
    debug_write("\nBlueOS riscv64 kernel entry\n");
    let _ = (hart_id, dtb); // Kept for the future device-tree parser.

    let mut gpu = VirtioGpu::probe();
    let framebuffer = match &gpu {
        Some(device) => device.framebuffer(),
        None => {
            debug_write("virtio-gpu not found; serial console only\n");
            Framebuffer::unavailable()
        }
    };
    let mut ui = Ui::new(framebuffer, "RISCV64");

    let mut network = VirtioNet::probe().map(|nic| {
        let stack = NetworkStack::new(nic.mac_address());
        (nic, stack)
    });
    if let Some((nic, stack)) = &mut network {
        stack.start(nic);
    }
    app::banner(&mut ui, "riscv64", network.is_some());
    app::run_lua_demo(&mut ui);
    ui.write("> ");
    if let Some(device) = &mut gpu {
        device.flush();
        let _ = ui.take_dirty();
    }

    let mut shell = Shell::new();
    loop {
        if let Some(byte) = uart_receive() {
            match shell.feed(byte) {
                FeedResult::Echo(byte) => {
                    let text = [byte];
                    ui.write(unsafe { core::str::from_utf8_unchecked(&text) });
                }
                FeedResult::Backspace => ui.write("\x08"),
                FeedResult::Ready => {
                    ui.write("\n");
                    app::execute(shell.line(), &mut ui, &mut network, "riscv64");
                    shell.reset();
                }
                FeedResult::Ignored => {}
            }
        }
        if let Some((nic, stack)) = &mut network {
            if let Some(event) = stack.poll(nic) {
                app::show_network_event(&mut ui, event);
            }
        }
        if ui.take_dirty() {
            if let Some(device) = &mut gpu {
                device.flush();
            }
        }
        idle();
    }
}

fn uart_init() {
    unsafe {
        // 16550: disable IRQs, 8N1, FIFO on. OpenSBI already selected a baud rate.
        write_volatile((UART + 1) as *mut u8, 0x00);
        write_volatile((UART + 3) as *mut u8, 0x03);
        write_volatile((UART + 2) as *mut u8, 0x07);
    }
}

fn uart_receive() -> Option<u8> {
    unsafe {
        if read_volatile((UART + 5) as *const u8) & 1 != 0 {
            Some(read_volatile(UART as *const u8))
        } else {
            None
        }
    }
}

fn uart_write(byte: u8) {
    for _ in 0..100_000 {
        unsafe {
            if read_volatile((UART + 5) as *const u8) & 0x20 != 0 {
                write_volatile(UART as *mut u8, byte);
                return;
            }
        }
    }
}

pub fn debug_write(text: &str) {
    for byte in text.bytes() {
        if byte == b'\n' {
            uart_write(b'\r');
        }
        uart_write(byte);
    }
}

pub fn emergency_write(text: &str) {
    debug_write(text);
}

pub fn idle() {
    // Polling drivers do not yet enable interrupts, so WFI would never resume.
    unsafe { asm!("nop", options(nomem, nostack)) }
}
