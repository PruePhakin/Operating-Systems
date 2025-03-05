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


use acpi::handler;
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
    // writeln!(Writer, "{} {}", vault[0] as char, vault[1] as char).unwrap();

    
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

    /*
     *   // print out values from heap allocation
     *   let x = Box::new(42);   
     *   let y = Box::new(24);
     *   writeln!(Writer, "x + y = {}", *x + *y).unwrap();
     *   writeln!(Writer, "{x:#p} {:?}", *x).unwrap();
     *   writeln!(Writer, "{y:#p} {:?}", *y).unwrap();
     *   
     *   writeln!(serial(), "Starting kernel...").unwrap();
     */


    start();

    // Interrupt descriptor table that will loop while handling interrupts
    let lapic_ptr = interrupts::init_apic(rsdp.expect("Failed to get RSDP address") as usize, physical_offset, &mut mapper, &mut frame_allocator);
    HandlerTable::new()
        .keyboard(key)
        .timer(tick)
        .startup(start)
        .start(lapic_ptr);

}


/*
 *   Game implementation start from here
 *
 *   - Main kernel up top will call on start function that initializes the game
 *   - Tick function will run in the background as a game loop updating the game state
 *   - Key function will handle keyboard interrupt inputs as the game progresses
 */

 // Mutex to allow for safe access to game objects from different functions
use spin::Mutex;
// Random number generator crate
use oorandom::Rand32;

// Player 
pub struct PLAYER {
    anchor_point_x: usize,
    anchor_point_y: usize,
    hitbox_x: usize,
    hitbox_y: usize,
    score: i32,
}
impl PLAYER {

    // Initialize player
    // Anchor is top left corner of player
    pub fn new(anchor_x: usize, anchor_y: usize) -> Self {
        PLAYER {
            anchor_point_x: anchor_x,
            anchor_point_y: anchor_y,
            hitbox_x: 10,
            hitbox_y: 100,
            score: 0,
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
            for y in self.anchor_point_y+self.hitbox_y..self.anchor_point_y+self.hitbox_y+10 {
                screenwriter().draw_pixel(x, y, 0, 0, 0);
            }
        }
    }

    // Move player down
    pub fn move_player_down(&mut self) {
        // Stop player from moving out of bounds
        if self.anchor_point_y == 700 {
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

// Determines how far the ball will move per tick
static STEP : usize = 5;

// Ball
// Anchor is center of ball
// We will use a gradient to calculate the next position of the ball every frame. The axis_direction will determine if the ball is moving left or right (Right is true. left is false)
pub struct BALL {
    anchor_point_x: usize,
    anchor_point_y: usize,
    hitbox_x: usize,
    hitbox_y: usize,
    trajectory_gradient: i32,
    axis_direction: bool,
}
impl BALL {

    // Initialize ball
    pub fn new() -> BALL {
        BALL {
            anchor_point_x: 640,
            anchor_point_y: 400,
            hitbox_x: 9,
            hitbox_y: 9,

            // Handles Pathing of ball
            trajectory_gradient: random_gradient(),
            axis_direction: random_direction(),
        }
    }

    // Draw ball
    pub fn draw_ball(&mut self) {
        
        let half_x = self.hitbox_x / 2;
        let half_y = self.hitbox_y / 2;

        // Draw ball around center point
        for i in (self.anchor_point_x - half_x)..(self.anchor_point_x + half_x) {
            for j in (self.anchor_point_y - half_y)..(self.anchor_point_y + half_y) {
                screenwriter().draw_pixel(i, j, 0xff, 0xff, 0xff);
            }
        }
    }

    // Clear ball from screen
    pub fn clear_ball(&mut self) {
        let half_x = self.hitbox_x / 2;
        let half_y = self.hitbox_y / 2;

        // Draw ball around center point in black
        for i in (self.anchor_point_x - half_x)..(self.anchor_point_x + half_x) {
            for j in (self.anchor_point_y - half_y)..(self.anchor_point_y + half_y) {
                screenwriter().draw_pixel(i, j, 0, 0, 0);
            }
        }
    }


    // Check collision with players and top/bottom of screen
    pub fn check_collision(&mut self, player1: &PLAYER, player2: &PLAYER) {
        let ball_left = self.anchor_point_x - (self.hitbox_x / 2);
        let ball_right = self.anchor_point_x + (self.hitbox_x / 2);
        let ball_top = self.anchor_point_y - (self.hitbox_y / 2);
        let ball_bottom = self.anchor_point_y + (self.hitbox_y / 2);
        
        // For a STEP of 5, we need a larger buffer to catch collisions
        let collision_buffer = 5;

        // Check collision with player 1
        let p1_left = player1.anchor_point_x;
        let p1_right = player1.anchor_point_x + player1.hitbox_x;
        let p1_top = player1.anchor_point_y;
        let p1_bottom = player1.anchor_point_y + player1.hitbox_y;

        // Check if ball overlaps with player 1 (with buffer for faster movement)
        if ball_right + collision_buffer >= p1_left && ball_left <= p1_right + collision_buffer && 
        ball_bottom >= p1_top && ball_top <= p1_bottom {
            // Only bounce if ball is approaching from the right
            if !self.axis_direction {
                // Clear ball at current position
                self.clear_ball();
                
                // Change direction
                self.axis_direction = !self.axis_direction;
                
                // Make sure ball is not stuck inside paddle
                self.anchor_point_x = p1_right + (self.hitbox_x / 2) + collision_buffer;
                
                // Redraw at new position
                self.draw_ball();
            }
        }

        // Check collision with player 2
        let p2_left = player2.anchor_point_x;
        let p2_right = player2.anchor_point_x + player2.hitbox_x;
        let p2_top = player2.anchor_point_y;
        let p2_bottom = player2.anchor_point_y + player2.hitbox_y;

        // Check if ball overlaps with player 2 (with buffer for faster movement)
        if ball_right >= p2_left - collision_buffer && ball_left - collision_buffer <= p2_right && 
        ball_bottom >= p2_top && ball_top <= p2_bottom {
            // Only bounce if ball is approaching from the left
            if self.axis_direction {
                // Clear ball at current position
                self.clear_ball();
                
                // Change direction
                self.axis_direction = !self.axis_direction;
                
                // Make sure ball is not stuck inside paddle
                self.anchor_point_x = p2_left - (self.hitbox_x / 2) - collision_buffer;
                
                // Redraw at new position
                self.draw_ball();
            }
        }

        // Check collision with top of screen
        if ball_top <= collision_buffer {
            // Only bounce if ball is moving upward
            if self.trajectory_gradient < 0 {
                // Clear any artifacts
                self.clear_ball();
                
                // Reverse trajectory
                self.trajectory_gradient = -self.trajectory_gradient;
                
                // Ensure the ball doesn't get stuck at the top boundary
                self.anchor_point_y = (self.hitbox_y / 2) + collision_buffer * 2;
                
                // Redraw at new position
                self.draw_ball();
            }
        }

        // Check collision with bottom of screen
        if ball_bottom >= 800 - collision_buffer {
            // Only bounce if ball is moving downward
            if self.trajectory_gradient > 0 {
                // Clear any artifacts
                self.clear_ball();
                
                // Reverse trajectory
                self.trajectory_gradient = -self.trajectory_gradient;
                
                // Ensure the ball doesn't get stuck at the bottom boundary
                self.anchor_point_y = 800 - (self.hitbox_y / 2) - collision_buffer * 2;
                
                // Redraw at new position
                self.draw_ball();
            }
        }
    }

    // Ball movement engine
    // This function will move the ball center point towards the next position using the gradient value. It will also redraw the previous position with black and redraw ball.
    pub fn update_ball(&mut self) {
        // Remove the previous ball position
        self.clear_ball();

        // Move horizontally based on direction
        if self.axis_direction {
            self.anchor_point_x += STEP; 
        } else {
            self.anchor_point_x -= STEP;
        }
        
        // Move vertically based on gradient (always add gradient regardless of direction)
        // This ensures consistent vertical movement
        if self.trajectory_gradient >= 0 {
            self.anchor_point_y += self.trajectory_gradient as usize;
        } else {
            // For negative gradients, we need to convert to positive for usize subtraction
            self.anchor_point_y -= (-self.trajectory_gradient) as usize;
        }

        // Draw the new ball position
        self.draw_ball();
    }

    // Check if the ball has gone out of bounds and update score then reset ball position
    pub fn check_score(&mut self) {
        let ball_left = self.anchor_point_x - (self.hitbox_x / 2);
        let ball_right = self.anchor_point_x + (self.hitbox_x / 2);
        let collision_buffer = STEP;


        // Check collision with left of screen
        if ball_left <= collision_buffer {
            // Player 1 scores
            if let Some(player1) = &mut *PLAYER1.lock() {
                player1.score += 1;
            }
            // Clear the old ball on screen
            self.clear_ball();

            // Reset ball position
            self.anchor_point_x = 640;
            self.anchor_point_y = 400;
            self.trajectory_gradient = random_gradient();
            self.axis_direction = random_direction();

            // Draw the new ball on screen
            self.draw_ball();

        }

        // Check collision with right of screen
        if ball_right >= 1280 - collision_buffer {
            // Player 2 scores
            if let Some(player2) = &mut *PLAYER2.lock() {
                player2.score += 1;
            }
            // Clear the old ball on screen
            self.clear_ball();

            // Reset ball position
            self.anchor_point_x = 640;
            self.anchor_point_y = 400;
            self.trajectory_gradient = random_gradient();
            self.axis_direction = random_direction();

            // Draw the new ball on screen
            self.draw_ball();
        }

    }
}



// Static variable to store frame buffer info
static mut FRAME_INFO: Option<FrameBufferInfo> = None;
static PLAYER1: Mutex<Option<PLAYER>> = Mutex::new(None);
static PLAYER2: Mutex<Option<PLAYER>> = Mutex::new(None);
static BALL: Mutex<Option<BALL>> = Mutex::new(None);


static RNG: Mutex<Option<Rand32>> = Mutex::new(None);

// Function to generate random numbers in range [-5, -1] or [1, 5] using oorandom
pub fn random_gradient() -> i32 {
    if let Some(rng) = &mut *RNG.lock() {
        // First decide if positive or negative (0 = negative, 1 = positive)
        let sign = (rng.rand_u32() % 2) as i32;
        
        // Generate number between 1 and 5
        let magnitude = ((rng.rand_u32() % 5) + 1) as i32;
        
        // Apply sign
        if sign == 0 {
            -magnitude  // Negative: -1 to -5
        } else {
            magnitude   // Positive: 1 to 5
        }
    } else {
        // Fallback if RNG not initialized
        1
    }
}

// Function to generate random direction (true = right, false = left) using oorandom
pub fn random_direction() -> bool {
    if let Some(rng) = &mut *RNG.lock() {
        // Generate 0 or 1
        (rng.rand_u32() % 2) == 1
    } else {
        // Fallback if RNG not initialized
        true
    }
}

// Will handle drawing initial state and setting up game objects
fn start() {
    // Initialize RNG
    let seed = 69;
    *RNG.lock() = Some(Rand32::new(seed));

    // Access the static frame_info (Static variable was assigned value in kernel_main)
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
    *PLAYER1.lock() = Some(PLAYER::new(70, 350));
    *PLAYER2.lock() = Some(PLAYER::new(1200, 350));
    *BALL.lock() = Some(BALL::new());

    // Draw game objects
    if let Some(player1) = &mut *PLAYER1.lock() {
        player1.draw_player();
    }
    if let Some(player2) = &mut *PLAYER2.lock() {
        player2.draw_player();
    }
    if let Some(ball) = &mut *BALL.lock() {
        ball.draw_ball();
    }

}

// Will handle updating the game state. This will mostly deal with ball movement and score updating
fn tick() {

    // Check if the ball has gone out of bounds
    if let Some(ball) = &mut *BALL.lock() {
        ball.check_score();
    }

    // Check collision
    if let Some(ball) = &mut *BALL.lock() {
        if let Some(player1) = &mut *PLAYER1.lock() {
            if let Some(player2) = &mut *PLAYER2.lock() {
                ball.check_collision(player1, player2);
            }
        }
    }

    // Update ball position
    if let Some(ball) = &mut *BALL.lock() {
        ball.update_ball();
    }

    // Access the static frame_info (Static variable was assigned value in kernel_main)
    let frame_info = unsafe { 
        FRAME_INFO.expect("Frame info not initialized")
    };
    // Redraw dotted center line
    for i in 0..frame_info.height {
        if i % 10 == 0 {
            screenwriter().draw_pixel(frame_info.width / 2, i, 0xff, 0xff, 0xff);
        }
    }


}

// Will mostly deal with user input to move the players
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