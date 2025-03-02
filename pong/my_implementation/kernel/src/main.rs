#![no_std]
#![no_main]

use bootloader_api::{entry_point, BootInfo, BootloaderConfig};
use panic_halt as _;// Adds a panic handler that halts the processor


// Define the bootloader configuration
const CONFIG: BootloaderConfig = {
    let mut config = BootloaderConfig::new_default();
    config.kernel_stack_size = 100 * 1024; // 100 KiB
    config
};
// Register the entry point with custom bootloader configuration from CONFIG
entry_point!(kernel_main, config = &CONFIG);


// Kernel entry point
fn kernel_main(_boot_info: &'static mut BootInfo) -> ! {
    // Your kernel code here
    loop {}
}
