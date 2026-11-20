//! Offscreen GLES for a hardware libretro core on Slot's emulation thread.
use crate::CoreError;
use libloading::Library;
use std::ffi::{c_char, c_void, CStr};
use std::ptr::{null, null_mut};
use std::time::{Duration, Instant};

type Handle = *mut c_void;
type Reset = unsafe extern "C" fn();
type Proc = unsafe extern "C" fn();

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct Callback {
    pub context: u32,
    pub reset: Option<Reset>,
    pub framebuffer: Option<unsafe extern "C" fn() -> usize>,
    pub proc_address: Option<unsafe extern "C" fn(*const c_char) -> Option<Proc>>,
    pub depth: bool,
    pub stencil: bool,
    pub bottom_left: bool,
    pub major: u32,
    pub minor: u32,
    pub cache_context: bool,
    pub destroy: Option<Reset>,
    pub debug_context: bool,
}

impl Callback {
    pub fn version(self) -> Option<i32> {
        match self.context {
            2 => Some(2), // RETRO_HW_CONTEXT_OPENGLES2
            4 => Some(3), // RETRO_HW_CONTEXT_OPENGLES3
            5 if matches!(self.major, 2 | 3) && self.minor == 0 => Some(self.major as i32),
            _ => None,
        }
    }
}

fn error(message: &str) -> CoreError {
    CoreError::Load(format!("hardware renderer: {message}"))
}

unsafe fn open(names: &[&str]) -> Result<Library, CoreError> {
    names
        .iter()
        .find_map(|name| Library::new(name).ok())
        .ok_or_else(|| error(names[0]))
}

macro_rules! entry_points {
    ($name:ident { $($entry:ident: ($($arg:ty),*) $(-> $ret:ty)?),+ $(,)? }) => {
        #[allow(non_snake_case)]
        struct $name { $($entry: unsafe extern "C" fn($($arg),*) $(-> $ret)?),+ }
        impl $name {
            unsafe fn load(lib: &Library) -> Result<Self, CoreError> {
                Ok(Self { $($entry: *lib.get(concat!(stringify!($entry), "\0").as_bytes())
                    .map_err(|_| error(stringify!($entry)))?),+ })
            }
        }
    };
}

entry_points!(Egl {
    eglGetDisplay: (Handle) -> Handle,
    eglInitialize: (Handle, *mut i32, *mut i32) -> u32,
    eglBindAPI: (u32) -> u32,
    eglChooseConfig: (Handle, *const i32, *mut Handle, i32, *mut i32) -> u32,
    eglCreatePbufferSurface: (Handle, Handle, *const i32) -> Handle,
    eglCreateContext: (Handle, Handle, Handle, *const i32) -> Handle,
    eglMakeCurrent: (Handle, Handle, Handle, Handle) -> u32,
    eglDestroyContext: (Handle, Handle) -> u32,
    eglDestroySurface: (Handle, Handle) -> u32,
    eglGetProcAddress: (*const c_char) -> Option<Proc>,
});

entry_points!(Gl {
    glGenFramebuffers: (i32, *mut u32),
    glBindFramebuffer: (u32, u32),
    glGenTextures: (i32, *mut u32),
    glBindTexture: (u32, u32),
    glTexParameteri: (u32, u32, i32),
    glTexImage2D: (u32, i32, i32, i32, i32, i32, u32, u32, *const c_void),
    glFramebufferTexture2D: (u32, u32, u32, u32, i32),
    glGenRenderbuffers: (i32, *mut u32),
    glBindRenderbuffer: (u32, u32),
    glRenderbufferStorage: (u32, u32, i32, i32),
    glFramebufferRenderbuffer: (u32, u32, u32, u32),
    glCheckFramebufferStatus: (u32) -> u32,
    glGetIntegerv: (u32, *mut i32),
    glGetString: (u32) -> *const c_char,
    glBindBuffer: (u32, u32),
    glPixelStorei: (u32, i32),
    glReadPixels: (i32, i32, i32, i32, u32, u32, *mut c_void),
});

const FB: u32 = 0x8d40;
const RB: u32 = 0x8d41;
const TEX: u32 = 0x0de1;
const RGBA: u32 = 0x1908;
const UBYTE: u32 = 0x1401;
const EGL_NONE: i32 = 0x3038;

pub(crate) struct Renderer {
    egl: Egl,
    gl: Gl,
    _egl_library: Library,
    gles: Library,
    display: Handle,
    surface: Handle,
    context: Handle,
    owner: std::thread::ThreadId,
    framebuffer: u32,
    version: i32,
    size: (u32, u32),
    scratch: Vec<u8>,
    timing: FrameTiming,
}

#[derive(Default)]
struct FrameTiming {
    window: Option<Instant>,
    runs: u32,
    frames: u32,
    run_time: Duration,
    copy_time: Duration,
}

// LibretroCore is sent to its worker before load. The renderer is created by
// load, remains current on that worker, and is destroyed there on unload.
// The owner check protects EGL from calls if a caller moves a loaded core.
unsafe impl Send for Renderer {}

impl Renderer {
    pub fn new(callback: Callback, width: u32, height: u32) -> Result<Self, CoreError> {
        let version = callback
            .version()
            .ok_or_else(|| error("unsupported GLES version"))?;
        unsafe {
            let egl_library = open(&["libEGL.so.1", "libEGL.so"])?;
            let gles = open(&["libGLESv2.so.2", "libGLESv2.so"])?;
            let egl = Egl::load(&egl_library)?;
            let gl = Gl::load(&gles)?;
            let display = (egl.eglGetDisplay)(null_mut());
            if display.is_null() || (egl.eglInitialize)(display, null_mut(), null_mut()) == 0 {
                return Err(error("eglInitialize failed"));
            }
            let mut result = Self {
                egl,
                gl,
                _egl_library: egl_library,
                gles,
                display,
                surface: null_mut(),
                context: null_mut(),
                owner: std::thread::current().id(),
                framebuffer: 0,
                version,
                size: (width, height),
                scratch: Vec::new(),
                timing: FrameTiming::default(),
            };
            let egl = &result.egl;
            let mut config = null_mut();
            let mut count = 0;
            let attributes = [
                0x3033,
                1,
                0x3040,
                if version == 3 { 0x40 } else { 4 },
                EGL_NONE,
            ];
            if (egl.eglBindAPI)(0x30a0) == 0
                || (egl.eglChooseConfig)(display, attributes.as_ptr(), &mut config, 1, &mut count)
                    == 0
                || count == 0
            {
                return Err(error("no offscreen EGL configuration"));
            }
            result.surface = (egl.eglCreatePbufferSurface)(
                display,
                config,
                [0x3057, 1, 0x3056, 1, EGL_NONE].as_ptr(),
            );
            result.context = (egl.eglCreateContext)(
                display,
                config,
                null_mut(),
                [0x3098, version, EGL_NONE].as_ptr(),
            );
            if result.surface.is_null() || result.context.is_null() || !result.make_current() {
                return Err(error("could not make the offscreen EGL context current"));
            }
            // Some EGL drivers return GLES 3 even when asked for a GLES 2 context.
            let reported = (result.gl.glGetString)(0x1f02); // GL_VERSION
            if !reported.is_null()
                && CStr::from_ptr(reported)
                    .to_bytes()
                    .starts_with(b"OpenGL ES 3")
            {
                result.version = 3;
            }
            result.allocate(callback)?;
            Ok(result)
        }
    }

    unsafe fn allocate(&mut self, callback: Callback) -> Result<(), CoreError> {
        let gl = &self.gl;
        let mut max = 0;
        (gl.glGetIntegerv)(0x0d33, &mut max); // GL_MAX_TEXTURE_SIZE
        let (w, h) = self.size;
        if max <= 0 || w == 0 || h == 0 || w > max as u32 || h > max as u32 {
            return Err(error("unsupported framebuffer size"));
        }
        let mut color = 0;
        (gl.glGenTextures)(1, &mut color);
        (gl.glBindTexture)(TEX, color);
        (gl.glTexParameteri)(TEX, 0x2801, 0x2600);
        (gl.glTexParameteri)(TEX, 0x2800, 0x2600);
        (gl.glTexImage2D)(
            TEX,
            0,
            RGBA as i32,
            w as i32,
            h as i32,
            0,
            RGBA,
            UBYTE,
            null(),
        );
        (gl.glGenFramebuffers)(1, &mut self.framebuffer);
        (gl.glBindFramebuffer)(FB, self.framebuffer);
        (gl.glFramebufferTexture2D)(FB, 0x8ce0, TEX, color, 0);
        if callback.depth || callback.stencil {
            let mut buffer = 0;
            (gl.glGenRenderbuffers)(1, &mut buffer);
            (gl.glBindRenderbuffer)(RB, buffer);
            (gl.glRenderbufferStorage)(
                RB,
                if callback.stencil { 0x88f0 } else { 0x81a5 },
                w as i32,
                h as i32,
            );
            (gl.glFramebufferRenderbuffer)(FB, 0x8d00, RB, buffer);
            if callback.stencil {
                (gl.glFramebufferRenderbuffer)(FB, 0x8d20, RB, buffer);
            }
        }
        if (gl.glCheckFramebufferStatus)(FB) != 0x8cd5 {
            return Err(error("incomplete framebuffer"));
        }
        (gl.glBindFramebuffer)(FB, 0);
        (gl.glBindRenderbuffer)(RB, 0);
        (gl.glBindTexture)(TEX, 0);
        Ok(())
    }

    pub fn make_current(&self) -> bool {
        self.owner == std::thread::current().id()
            && unsafe {
                (self.egl.eglMakeCurrent)(self.display, self.surface, self.surface, self.context)
                    != 0
            }
    }

    pub fn framebuffer(&self) -> usize {
        self.framebuffer as usize
    }

    pub unsafe fn proc_address(&self, name: &CStr) -> Option<Proc> {
        (self.egl.eglGetProcAddress)(name.as_ptr()).or_else(|| {
            self.gles
                .get::<Proc>(name.to_bytes_with_nul())
                .ok()
                .map(|symbol| *symbol)
        })
    }

    pub fn copy_frame(&mut self, w: u32, h: u32, flip: bool, canvas: (u32, u32), video: &mut [u8]) {
        let started = Instant::now();
        let w = w.min(self.size.0) as usize;
        let h = h.min(self.size.1) as usize;
        if w == 0 || h == 0 || self.owner != std::thread::current().id() {
            return;
        }
        self.scratch.resize(w * h * 4, 0);
        unsafe {
            let gl = &self.gl;
            let mut previous_fb = 0;
            let mut pbo = 0;
            let mut pack = [0; 4];
            let pack_settings = [0x0d05, 0x0d02, 0x0d03, 0x0d04]; // alignment, row length, skipped rows/pixels
            let count = if self.version >= 3 { 4 } else { 1 };
            let (target, binding) = if self.version >= 3 {
                (0x8ca8, 0x8caa)
            } else {
                (FB, 0x8ca6)
            };
            (gl.glGetIntegerv)(binding, &mut previous_fb);
            if self.version >= 3 {
                (gl.glGetIntegerv)(0x88ed, &mut pbo);
                (gl.glBindBuffer)(0x88eb, 0);
            }
            for (i, (&setting, value)) in
                pack_settings.iter().zip(&mut pack).take(count).enumerate()
            {
                (gl.glGetIntegerv)(setting, value);
                (gl.glPixelStorei)(setting, i32::from(i == 0));
            }
            (gl.glBindFramebuffer)(target, self.framebuffer);
            (gl.glReadPixels)(
                0,
                0,
                w as i32,
                h as i32,
                RGBA,
                UBYTE,
                self.scratch.as_mut_ptr().cast(),
            );
            (gl.glBindFramebuffer)(target, previous_fb as u32);
            for (&setting, value) in pack_settings.iter().zip(pack).take(count) {
                (gl.glPixelStorei)(setting, value);
            }
            if self.version >= 3 {
                (gl.glBindBuffer)(0x88eb, pbo as u32);
            }
        }
        let (cw, ch) = (canvas.0 as usize, canvas.1 as usize);
        for y in 0..ch {
            let sy = y * h / ch;
            let sy = if flip { h - 1 - sy } else { sy };
            for x in 0..cw {
                let i = (sy * w + x * w / cw) * 4;
                let j = (y * cw + x) * 4;
                let p = &self.scratch[i..i + 4];
                video[j..j + 4].copy_from_slice(&[p[2], p[1], p[0], 255]);
            }
        }
        self.timing.frames += 1;
        self.timing.copy_time += started.elapsed();
    }

    pub fn record_run(&mut self, started: Instant) {
        let timing = &mut self.timing;
        let elapsed = timing.window.get_or_insert(started).elapsed();
        timing.runs += 1;
        timing.run_time += started.elapsed();
        if elapsed < Duration::from_secs(5) {
            return;
        }
        eprintln!(
            "slot: PSP timing: {:.1} runs/s, {:.1} frames/s, {:.2} ms/run total, {:.2} ms/frame readback and copy",
            f64::from(timing.runs) / elapsed.as_secs_f64(),
            f64::from(timing.frames) / elapsed.as_secs_f64(),
            timing.run_time.as_secs_f64() * 1000.0 / f64::from(timing.runs),
            timing.copy_time.as_secs_f64() * 1000.0 / f64::from(timing.frames.max(1)),
        );
        *timing = FrameTiming::default();
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        if self.owner != std::thread::current().id() {
            return;
        }
        unsafe {
            if !self.context.is_null() {
                (self.egl.eglMakeCurrent)(self.display, null_mut(), null_mut(), null_mut());
                (self.egl.eglDestroyContext)(self.display, self.context);
            }
            if !self.surface.is_null() {
                (self.egl.eglDestroySurface)(self.display, self.surface);
            }
        }
        // The display is shared with Slot's UI. Terminating it would destroy the UI context.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Run explicitly with EGL_PLATFORM=surfaceless on a Mesa build host.
    #[test]
    #[ignore = "requires an EGL/GLES runtime"]
    fn offscreen_context_reads_a_rendered_frame() {
        let cb = Callback {
            context: 2,
            reset: None,
            framebuffer: None,
            proc_address: None,
            depth: true,
            stencil: true,
            bottom_left: true,
            major: 2,
            minor: 0,
            cache_context: false,
            destroy: None,
            debug_context: false,
        };
        let mut renderer = Renderer::new(cb, 2, 2).unwrap();
        unsafe {
            let clear_color = *renderer
                .gles
                .get::<unsafe extern "C" fn(f32, f32, f32, f32)>(b"glClearColor\0")
                .unwrap();
            let clear = *renderer
                .gles
                .get::<unsafe extern "C" fn(u32)>(b"glClear\0")
                .unwrap();
            (renderer.gl.glBindFramebuffer)(FB, renderer.framebuffer);
            clear_color(1.0, 0.0, 0.0, 1.0);
            clear(0x4000);
        }
        let mut video = [0; 16];
        renderer.copy_frame(2, 2, true, (2, 2), &mut video);
        assert_eq!(video, [0, 0, 255, 255].repeat(4).as_slice());
    }
}
