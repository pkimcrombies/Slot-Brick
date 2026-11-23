use crate::quad::Quad;
use crate::surface::{GfxError, OUT_H, OUT_W};

// The desktop GL core bindings omit this GLES 2 texture format.
const GL_LUMINANCE: gl::types::GLenum = 0x1909;

const VIDEO_FRAG: &str = r#"
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif
uniform sampler2D u_y;
uniform sampler2D u_cb;
uniform sampler2D u_cr;
uniform vec2 u_chroma_scale;
varying vec2 v_uv;
void main() {
    vec2 uv = v_uv * u_chroma_scale;
    float y = max(texture2D(u_y, v_uv).r - 16.0 / 255.0, 0.0);
    float cb = texture2D(u_cb, uv).r - 128.0 / 255.0;
    float cr = texture2D(u_cr, uv).r - 128.0 / 255.0;
    // Limited-range BT.601, matching Slot's original CPU conversion.
    vec3 rgb = (vec3(298.0 * y)
        + vec3(409.0, -208.0, 0.0) * cr
        + vec3(0.0, -100.0, 516.0) * cb) / 256.0;
    FRAG_COLOR = vec4(clamp(rgb, 0.0, 1.0), 1.0);
}
"#;

pub(crate) struct VideoPass {
    program: gl::types::GLuint,
    textures: [gl::types::GLuint; 3],
    rect: gl::types::GLint,
    chroma_scale: gl::types::GLint,
    size: Option<(u32, u32)>,
}

impl VideoPass {
    pub(crate) fn new() -> Result<Self, GfxError> {
        let program = crate::shaders::program(crate::shaders::RECT_VERT, VIDEO_FRAG)?;
        let uniform = |name| crate::gl::uniform_location(program, name);
        let mut textures = [0; 3];
        unsafe {
            gl::GenTextures(3, textures.as_mut_ptr());
            gl::UseProgram(program);
            gl::Uniform1i(uniform("u_y"), 0);
            gl::Uniform1i(uniform("u_cb"), 1);
            gl::Uniform1i(uniform("u_cr"), 2);
            gl::Uniform2f(uniform("u_target"), OUT_W as f32, OUT_H as f32);
        }
        Ok(Self {
            program,
            textures,
            rect: uniform("u_rect"),
            chroma_scale: uniform("u_chroma_scale"),
            size: None,
        })
    }

    pub(crate) fn upload(&mut self, w: u32, h: u32, planes: [&[u8]; 3]) -> Result<(), GfxError> {
        let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
        let dimensions = [(w, h), (cw, ch), (cw, ch)];
        if w == 0 || h == 0 || dimensions.iter().zip(planes).any(|(&(w, h), plane)| {
            (w as usize).checked_mul(h as usize) != Some(plane.len())
        }) {
            return Err(GfxError::Context("invalid video plane dimensions".into()));
        }

        let resize = self.size != Some((w, h));
        let (internal, format) = if crate::gl::es() {
            (GL_LUMINANCE, GL_LUMINANCE)
        } else {
            (gl::R8, gl::RED)
        };
        unsafe {
            gl::ActiveTexture(gl::TEXTURE0);
            gl::PixelStorei(gl::UNPACK_ALIGNMENT, 1);
            if resize {
                // Discard any error left by an unrelated draw before checking
                // allocation on this driver. Same-size updates need no check.
                gl::GetError();
            }
            for (index, ((texture, (pw, ph)), plane)) in self.textures
                .into_iter().zip(dimensions).zip(planes).enumerate()
            {
                gl::BindTexture(gl::TEXTURE_2D, texture);
                let (pw, ph) = (pw as i32, ph as i32);
                if resize {
                    let filter = if index == 0 { gl::LINEAR } else { gl::NEAREST };
                    gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, filter as i32);
                    gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, filter as i32);
                    gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, gl::CLAMP_TO_EDGE as i32);
                    gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, gl::CLAMP_TO_EDGE as i32);
                    gl::TexImage2D(gl::TEXTURE_2D, 0, internal as i32, pw, ph, 0,
                        format, gl::UNSIGNED_BYTE, plane.as_ptr().cast());
                } else {
                    gl::TexSubImage2D(gl::TEXTURE_2D, 0, 0, 0, pw, ph,
                        format, gl::UNSIGNED_BYTE, plane.as_ptr().cast());
                }
            }
            if resize {
                let error = gl::GetError();
                if error != gl::NO_ERROR {
                    self.size = None;
                    return Err(GfxError::Context(format!("video texture upload failed ({error:#x})")));
                }
                gl::UseProgram(self.program);
                // The last chroma sample also covers a lone pixel on odd dimensions.
                gl::Uniform2f(self.chroma_scale, w as f32 / (2.0 * cw as f32),
                    h as f32 / (2.0 * ch as f32));
                self.size = Some((w, h));
            }
        }
        Ok(())
    }

    pub(crate) fn draw(&self, rect: [f32; 4], quad: &Quad) {
        if self.size.is_none() {
            return;
        }
        unsafe {
            gl::Disable(gl::BLEND);
            gl::UseProgram(self.program);
            gl::Uniform4fv(self.rect, 1, rect.as_ptr());
            for (index, texture) in self.textures.into_iter().enumerate() {
                gl::ActiveTexture(gl::TEXTURE0 + index as u32);
                gl::BindTexture(gl::TEXTURE_2D, texture);
            }
            quad.draw();
            gl::ActiveTexture(gl::TEXTURE0);
        }
    }
}

impl Drop for VideoPass {
    fn drop(&mut self) {
        unsafe {
            gl::DeleteTextures(3, self.textures.as_ptr());
            gl::DeleteProgram(self.program);
        }
    }
}
