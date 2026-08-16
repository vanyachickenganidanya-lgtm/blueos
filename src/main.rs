#![no_std]
#![no_main]

mod app;
mod framebuffer;
mod font;
mod lua;
mod net;
mod shell;

#[cfg(target_arch = "x86_64")]
#[path = "arch/x86_64.rs"]
mod arch;

#[cfg(target_arch = "riscv64")]
#[path = "arch/riscv64.rs"]
mod arch;

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    arch::emergency_write("\nBLUEOS PANIC\n");
    loop {
        arch::idle();
    }
}
