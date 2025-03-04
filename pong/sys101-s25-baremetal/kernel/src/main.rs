#![feature(sync_unsafe_cell)]
#![feature(abi_x86_interrupt)]
#![no_std] // don't link the Rust standard library
#![no_main] // disable all Rust-level entry points

extern crate alloc;

mod screen;
mod allocator;
mod frame_allocator;
mod interrupts;
mod gdt;


use alloc::boxed::Box;
use x86_64::structures::paging::frame;
use core::fmt::Write;
use core::panic::Location;
use core::slice;
use bootloader_api::{entry_point, BootInfo, BootloaderConfig};
use bootloader_api::config::Mapping::Dynamic;
use bootloader_api::info::{FrameBufferInfo, MemoryRegionKind};
use kernel::{HandlerTable, serial};
use pc_keyboard::{DecodedKey, KeyCode};
use x86_64::registers::control::Cr3;
use x86_64::VirtAddr;
use crate::frame_allocator::BootInfoFrameAllocator;
use crate::screen::{Writer, screenwriter};

use spin::Mutex;

// Define the bootloader configuration
const BOOTLOADER_CONFIG: BootloaderConfig = {
    let mut config = BootloaderConfig::new_default();
    config.mappings.physical_memory = Some(Dynamic); // obtain physical memory offset
    config.kernel_stack_size = 256 * 1024; // 256 KiB kernel stack size
    config
};
// Define the entry point to be kernel_main
entry_point!(kernel_main, config = &BOOTLOADER_CONFIG);


// Entry point
fn kernel_main(boot_info: &'static mut BootInfo) -> ! {


    writeln!(serial(), "Entered kernel with boot info: {boot_info:?}").unwrap();
    writeln!(serial(), "Frame Buffer: {:p}", boot_info.framebuffer.as_ref().unwrap().buffer()).unwrap();
    

    // Initialize the screen
    let frame_info = boot_info.framebuffer.as_ref().unwrap().info();
    // Store frame_info in static variable
    unsafe {
        FRAME_INFO = Some(frame_info.clone());
    }
    let framebuffer = boot_info.framebuffer.as_mut().unwrap();
    screen::init(framebuffer);
        
    for r in boot_info.memory_regions.iter() {
        writeln!(serial(), "{:?} {:?} {:?} {}", r, r.start as *mut u8, r.end as *mut usize, r.end-r.start).unwrap();
    }

    let usable_region = boot_info.memory_regions.iter().filter(|x|x.kind == MemoryRegionKind::Usable).last().unwrap();
    writeln!(serial(), "{usable_region:?}").unwrap();

    let physical_offset = boot_info.physical_memory_offset.take().expect("Failed to find physical memory offset");
    let ptr = (physical_offset + usable_region.start) as *mut u8;
    writeln!(serial(), "Physical memory offset: {:X}; usable range: {:p}", physical_offset, ptr).unwrap();

    // print out values stored in specific memory address
    let vault = unsafe { slice::from_raw_parts_mut(ptr, 100) };
    vault[0] = 65;
    vault[1] = 66;
    writeln!(Writer, "{} {}", vault[0] as char, vault[1] as char).unwrap();

    
    //read CR3 for current page table
    let cr3 = Cr3::read().0.start_address().as_u64();
    writeln!(serial(), "CR3 read: {:#x}", cr3).unwrap();

    let cr3_page = unsafe { slice::from_raw_parts_mut((cr3 + physical_offset) as *mut usize, 6) };
    writeln!(serial(), "CR3 Page table virtual address {cr3_page:#p}").unwrap();

    allocator::init_heap((physical_offset + usable_region.start) as usize);

    let rsdp = boot_info.rsdp_addr.take();
    let mut mapper = frame_allocator::init(VirtAddr::new(physical_offset));
    let mut frame_allocator = BootInfoFrameAllocator::new(&boot_info.memory_regions);
    
    gdt::init();

    // print out values from heap allocation
    let x = Box::new(42);   
    let y = Box::new(24);
    writeln!(Writer, "x + y = {}", *x + *y).unwrap();
    writeln!(Writer, "{x:#p} {:?}", *x).unwrap();
    writeln!(Writer, "{y:#p} {:?}", *y).unwrap();
    
    writeln!(serial(), "Starting kernel...").unwrap();

    start();

    // Interrupt descriptor table that will loop while handling interrupts
    let lapic_ptr = interrupts::init_apic(rsdp.expect("Failed to get RSDP address") as usize, physical_offset, &mut mapper, &mut frame_allocator);
    HandlerTable::new()
        .keyboard(key)
        .timer(tick)
        .startup(start)
        .start(lapic_ptr);

}

// Player 
pub struct PLAYER {
    anchor_point_x: usize,
    anchor_point_y: usize,
    hitbox_x: usize,
    hitbox_y: usize,
}
impl PLAYER {

    // Initialize player
    // Anchor is top left corner of player
    pub fn new(anchor_x: usize, anchor_y: usize) -> Self {
        PLAYER {
            anchor_point_x: anchor_x,
            anchor_point_y: anchor_y,
            hitbox_x: 10,
            hitbox_y: 90,
        }
    }

    // Draw player
    pub fn draw_player(&mut self) {
        for i in self.anchor_point_x..self.anchor_point_x+self.hitbox_x {
            for j in self.anchor_point_y..self.anchor_point_y+self.hitbox_y {
                screenwriter().draw_pixel(i, j, 0xff, 0xff, 0xff);
            }
        }
    }

    // Move player up
    pub fn move_player_up(&mut self) {
        // Stop player from moving out of bounds
        if self.anchor_point_y == 0{
            return;
        }
        self.anchor_point_y -= 10;
        // Redraw the previous position with black
        for x in self.anchor_point_x..self.anchor_point_x+self.hitbox_x {
            for y in self.anchor_point_y+90..self.anchor_point_y+100 {
                screenwriter().draw_pixel(x, y, 0, 0, 0);
            }
        }
    }

    // Move player down
    pub fn move_player_down(&mut self) {
        // Stop player from moving out of bounds
        if self.anchor_point_y == 710 {
            return;
        }
        self.anchor_point_y += 10;

        // Redraw the previous position with black
        for x in self.anchor_point_x..self.anchor_point_x+self.hitbox_x {
            for y in self.anchor_point_y-10..self.anchor_point_y {
                screenwriter().draw_pixel(x, y, 0, 0, 0);
            }
        }
    }

}

// Ball
pub struct BALL {
    hitbox_x: i32,
    hitbox_y: i32,
}
impl BALL {
    pub fn new() -> BALL {
        BALL {
            hitbox_x: 0,
            hitbox_y: 0,
        }
    }
}

// Static variable to store frame buffer info
static mut FRAME_INFO: Option<FrameBufferInfo> = None;
static PLAYER1: Mutex<Option<PLAYER>> = Mutex::new(None);
static PLAYER2: Mutex<Option<PLAYER>> = Mutex::new(None);
static BALL: Mutex<Option<BALL>> = Mutex::new(None);


// Game implementation in here, start for initialization, tick for game loop, key for keyboard input
fn start() {

    // Access the static frame_info
    let frame_info = unsafe { 
        FRAME_INFO.expect("Frame info not initialized")
    };

    // print out screen size 1280x800
    writeln!(serial(), "Screen size: {}x{}", frame_info.width, frame_info.height).unwrap();

    // Draw dotted center line
    for i in 0..frame_info.height {
        if i % 10 == 0 {
            screenwriter().draw_pixel(frame_info.width / 2, i, 0xff, 0xff, 0xff);
        }
    }


    // Initialize game objects
    *PLAYER1.lock() = Some(PLAYER::new(70, 300));
    *PLAYER2.lock() = Some(PLAYER::new(1200, 300));
    *BALL.lock() = Some(BALL::new());

    // Draw objects
    if let Some(player1) = &mut *PLAYER1.lock() {
        player1.draw_player();
    }
    if let Some(player2) = &mut *PLAYER2.lock() {
        player2.draw_player();
    }

}


fn tick() {



    write!(Writer, ".").unwrap();
    write!(Writer, "-").unwrap();
}

// Keyboard input handler
fn key(key: DecodedKey) {
    match key {
        // Move player 1 up
        DecodedKey::Unicode('w') => {
            if let Some(player1) = &mut *PLAYER1.lock() {
                player1.move_player_up();
                player1.draw_player();
            }
        },
        // Move player 1 down
        DecodedKey::Unicode('s') => {
            if let Some(player1) = &mut *PLAYER1.lock() {
                player1.move_player_down();
                player1.draw_player();
            }
        },
        // Move player 2 up
        DecodedKey::RawKey(KeyCode::ArrowUp) => {
            if let Some(player2) = &mut *PLAYER2.lock() {
                player2.move_player_up();
                player2.draw_player();
            }
        },
        // Move player 2 down  
        DecodedKey::RawKey(KeyCode::ArrowDown) => {
            if let Some(player2) = &mut *PLAYER2.lock() {
                player2.move_player_down();
                player2.draw_player();
            }
        },
        DecodedKey::Unicode(character) => write!(Writer, "{}", character).unwrap(),
        DecodedKey::RawKey(key) => write!(Writer, "{:?}", key).unwrap(),
    }
}