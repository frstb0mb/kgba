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
const SDL_RENDERER_PRESENTVSYNC: u32 = 0x0000_0004;
const SDL_TEXTUREACCESS_STREAMING: c_int = 1;
const SDL_PIXELFORMAT_ARGB8888: u32 = 372645892;
const SDL_QUIT: u32 = 0x100;

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

            let mut renderer = SDL_CreateRenderer(
                window,
                -1,
                SDL_RENDERER_ACCELERATED | SDL_RENDERER_PRESENTVSYNC,
            );
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
                SDL_PIXELFORMAT_ARGB8888,
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

    pub fn present(&mut self, frame: &[u32]) -> Result<(), String> {
        unsafe {
            if SDL_UpdateTexture(
                self.texture,
                ptr::null(),
                frame.as_ptr().cast(),
                (WIDTH * 4) as c_int,
            ) != 0
            {
                return Err(sdl_error());
            }
            SDL_RenderClear(self.renderer);
            SDL_RenderCopy(self.renderer, self.texture, ptr::null(), ptr::null());
            SDL_RenderPresent(self.renderer);
        }
        Ok(())
    }

    pub fn run_until_quit(&mut self, frame: &[u32], minimum: Duration) -> Result<(), String> {
        let started = Instant::now();
        loop {
            self.present(frame)?;
            if started.elapsed() >= minimum && self.poll_quit() {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(16));
        }
    }

    pub fn run_frame_loop<F>(&mut self, mut next_frame: F) -> Result<(), String>
    where
        F: FnMut(u16) -> Vec<u32>,
    {
        let mut vcount = 0u16;
        loop {
            let frame = next_frame(vcount);
            self.present(&frame)?;
            if self.poll_quit() {
                return Ok(());
            }
            vcount = if vcount + 1 >= 228 { 0 } else { vcount + 1 };
            std::thread::sleep(Duration::from_millis(16));
        }
    }

    fn poll_quit(&mut self) -> bool {
        unsafe {
            let mut event = SDL_Event {
                type_: 0,
                padding: [0; 52],
            };
            while SDL_PollEvent(&mut event) != 0 {
                if event.type_ == SDL_QUIT {
                    return true;
                }
            }
        }
        false
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

unsafe fn sdl_error() -> String {
    let ptr = unsafe { SDL_GetError() };
    if ptr.is_null() {
        return "SDL error".to_owned();
    }
    unsafe { std::ffi::CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned()
}
