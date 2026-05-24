use std::{
    ffi::{CString, c_char, c_int, c_void},
    ptr,
    time::{Duration, Instant},
};

use crate::gba::ppu::{HEIGHT, WIDTH};

const SDL_INIT_VIDEO: u32 = 0x0000_0020;
const SDL_WINDOW_SHOWN: u32 = 0x0000_0004;
const SDL_RENDERER_SOFTWARE: u32 = 0x0000_0001;
const SDL_RENDERER_ACCELERATED: u32 = 0x0000_0002;
const SDL_TEXTUREACCESS_STREAMING: c_int = 1;
const SDL_PIXELFORMAT_BGR555: u32 = 357_764_866;
const SDL_QUIT: u32 = 0x100;
const FRAME_INTERVAL: Duration = Duration::from_micros(16_742);
const INPUT_POLL_INTERVAL: Duration = Duration::from_millis(1);

const SDL_SCANCODE_A: usize = 4;
const SDL_SCANCODE_D: usize = 7;
const SDL_SCANCODE_I: usize = 12;
const SDL_SCANCODE_K: usize = 14;
const SDL_SCANCODE_L: usize = 15;
const SDL_SCANCODE_O: usize = 18;
const SDL_SCANCODE_S: usize = 22;
const SDL_SCANCODE_W: usize = 26;
const SDL_SCANCODE_RETURN: usize = 40;
const SDL_SCANCODE_BACKSPACE: usize = 42;

const KEY_A: u16 = 1 << 0;
const KEY_B: u16 = 1 << 1;
const KEY_SELECT: u16 = 1 << 2;
const KEY_START: u16 = 1 << 3;
const KEY_RIGHT: u16 = 1 << 4;
const KEY_LEFT: u16 = 1 << 5;
const KEY_UP: u16 = 1 << 6;
const KEY_DOWN: u16 = 1 << 7;
const KEY_R: u16 = 1 << 8;
const KEY_L: u16 = 1 << 9;
const KEYINPUT_RELEASED: u16 = 0x03ff;

#[repr(C)]
struct SDL_Window(c_void);
#[repr(C)]
struct SDL_Renderer(c_void);
#[repr(C)]
struct SDL_Texture(c_void);

#[repr(C)]
#[derive(Clone, Copy)]
struct SDL_Event {
    type_: u32,
    padding: [u8; 52],
}

#[link(name = "SDL2")]
unsafe extern "C" {
    fn SDL_Init(flags: u32) -> c_int;
    fn SDL_Quit();
    fn SDL_GetError() -> *const c_char;
    fn SDL_CreateWindow(
        title: *const c_char,
        x: c_int,
        y: c_int,
        w: c_int,
        h: c_int,
        flags: u32,
    ) -> *mut SDL_Window;
    fn SDL_DestroyWindow(window: *mut SDL_Window);
    fn SDL_CreateRenderer(window: *mut SDL_Window, index: c_int, flags: u32) -> *mut SDL_Renderer;
    fn SDL_DestroyRenderer(renderer: *mut SDL_Renderer);
    fn SDL_RenderSetLogicalSize(renderer: *mut SDL_Renderer, w: c_int, h: c_int) -> c_int;
    fn SDL_CreateTexture(
        renderer: *mut SDL_Renderer,
        format: u32,
        access: c_int,
        w: c_int,
        h: c_int,
    ) -> *mut SDL_Texture;
    fn SDL_DestroyTexture(texture: *mut SDL_Texture);
    fn SDL_UpdateTexture(
        texture: *mut SDL_Texture,
        rect: *const c_void,
        pixels: *const c_void,
        pitch: c_int,
    ) -> c_int;
    fn SDL_RenderClear(renderer: *mut SDL_Renderer) -> c_int;
    fn SDL_RenderCopy(
        renderer: *mut SDL_Renderer,
        texture: *mut SDL_Texture,
        srcrect: *const c_void,
        dstrect: *const c_void,
    ) -> c_int;
    fn SDL_RenderPresent(renderer: *mut SDL_Renderer);
    fn SDL_PollEvent(event: *mut SDL_Event) -> c_int;
    fn SDL_PumpEvents();
    fn SDL_GetKeyboardState(numkeys: *mut c_int) -> *const u8;
}

pub struct Video {
    window: *mut SDL_Window,
    renderer: *mut SDL_Renderer,
    texture: *mut SDL_Texture,
}

impl Video {
    pub fn new(title: &str) -> Result<Self, String> {
        unsafe {
            if SDL_Init(SDL_INIT_VIDEO) != 0 {
                return Err(sdl_error());
            }

            let title = CString::new(title).map_err(|err| err.to_string())?;
            let window = SDL_CreateWindow(
                title.as_ptr(),
                100,
                100,
                (WIDTH * 3) as c_int,
                (HEIGHT * 3) as c_int,
                SDL_WINDOW_SHOWN,
            );
            if window.is_null() {
                return Err(sdl_error());
            }

            let mut renderer = SDL_CreateRenderer(window, -1, SDL_RENDERER_ACCELERATED);
            if renderer.is_null() {
                renderer = SDL_CreateRenderer(window, -1, SDL_RENDERER_SOFTWARE);
            }
            if renderer.is_null() {
                SDL_DestroyWindow(window);
                return Err(sdl_error());
            }
            SDL_RenderSetLogicalSize(renderer, WIDTH as c_int, HEIGHT as c_int);

            let texture = SDL_CreateTexture(
                renderer,
                SDL_PIXELFORMAT_BGR555,
                SDL_TEXTUREACCESS_STREAMING,
                WIDTH as c_int,
                HEIGHT as c_int,
            );
            if texture.is_null() {
                SDL_DestroyRenderer(renderer);
                SDL_DestroyWindow(window);
                return Err(sdl_error());
            }

            Ok(Self {
                window,
                renderer,
                texture,
            })
        }
    }

    pub fn present(&mut self, frame: &[u16]) -> Result<(), String> {
        self.present_timed(frame).map(|_| ())
    }

    pub fn present_timed(&mut self, frame: &[u16]) -> Result<Duration, String> {
        let started = Instant::now();
        unsafe {
            if SDL_UpdateTexture(
                self.texture,
                ptr::null(),
                frame.as_ptr().cast(),
                (WIDTH * 2) as c_int,
            ) != 0
            {
                return Err(sdl_error());
            }
            SDL_RenderClear(self.renderer);
            SDL_RenderCopy(self.renderer, self.texture, ptr::null(), ptr::null());
            SDL_RenderPresent(self.renderer);
        }
        Ok(started.elapsed())
    }

    pub fn run_until_quit(&mut self, frame: &[u16], minimum: Duration) -> Result<(), String> {
        let started = Instant::now();
        loop {
            self.present(frame)?;
            if started.elapsed() >= minimum && self.poll_quit() {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(16));
        }
    }

    pub fn run_frame_loop<F, I>(
        &mut self,
        mut present_frame: F,
        mut publish_input: I,
    ) -> Result<(), String>
    where
        F: FnMut(&mut Self, u16) -> Result<(), String>,
        I: FnMut(u16),
    {
        let mut vcount = 0u16;
        let mut next_present = Instant::now();
        loop {
            let (quit, keyinput) = self.poll_events_and_input();
            publish_input(keyinput);
            if quit {
                return Ok(());
            }

            let now = Instant::now();
            if now >= next_present {
                present_frame(self, vcount)?;
                vcount = if vcount + 1 >= 228 { 0 } else { vcount + 1 };
                next_present += FRAME_INTERVAL;
                if next_present < now {
                    next_present = now + FRAME_INTERVAL;
                }
            }

            std::thread::sleep(INPUT_POLL_INTERVAL);
        }
    }

    fn poll_quit(&mut self) -> bool {
        self.poll_events_and_input().0
    }

    pub fn poll_events_and_input(&mut self) -> (bool, u16) {
        unsafe {
            let mut event = SDL_Event {
                type_: 0,
                padding: [0; 52],
            };
            let mut quit = false;
            while SDL_PollEvent(&mut event) != 0 {
                if event.type_ == SDL_QUIT {
                    quit = true;
                }
            }
            SDL_PumpEvents();
            (quit, read_keyinput())
        }
    }
}

impl Drop for Video {
    fn drop(&mut self) {
        unsafe {
            SDL_DestroyTexture(self.texture);
            SDL_DestroyRenderer(self.renderer);
            SDL_DestroyWindow(self.window);
            SDL_Quit();
        }
    }
}

fn read_keyinput() -> u16 {
    let mut numkeys = 0;
    let state = unsafe { SDL_GetKeyboardState(&mut numkeys) };
    if state.is_null() {
        return KEYINPUT_RELEASED;
    }

    let keys = unsafe { std::slice::from_raw_parts(state, numkeys as usize) };
    let mut keyinput = KEYINPUT_RELEASED;

    clear_if_pressed(keys, SDL_SCANCODE_L, &mut keyinput, KEY_A);
    clear_if_pressed(keys, SDL_SCANCODE_K, &mut keyinput, KEY_B);
    clear_if_pressed(keys, SDL_SCANCODE_BACKSPACE, &mut keyinput, KEY_SELECT);
    clear_if_pressed(keys, SDL_SCANCODE_RETURN, &mut keyinput, KEY_START);
    clear_if_pressed(keys, SDL_SCANCODE_D, &mut keyinput, KEY_RIGHT);
    clear_if_pressed(keys, SDL_SCANCODE_A, &mut keyinput, KEY_LEFT);
    clear_if_pressed(keys, SDL_SCANCODE_W, &mut keyinput, KEY_UP);
    clear_if_pressed(keys, SDL_SCANCODE_S, &mut keyinput, KEY_DOWN);
    clear_if_pressed(keys, SDL_SCANCODE_O, &mut keyinput, KEY_R);
    clear_if_pressed(keys, SDL_SCANCODE_I, &mut keyinput, KEY_L);

    keyinput
}

fn clear_if_pressed(keys: &[u8], scancode: usize, keyinput: &mut u16, bit: u16) {
    if keys.get(scancode).copied().unwrap_or(0) != 0 {
        *keyinput &= !bit;
    }
}

unsafe fn sdl_error() -> String {
    let ptr = unsafe { SDL_GetError() };
    if ptr.is_null() {
        return "SDL error".to_owned();
    }
    unsafe { std::ffi::CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned()
}
