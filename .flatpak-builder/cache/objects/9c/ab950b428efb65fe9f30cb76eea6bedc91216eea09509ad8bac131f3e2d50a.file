//! Minimal OpenGL quad renderer for displaying image textures.
//! Draws a textured quad that fits the image within the viewport while preserving aspect ratio.

use std::ffi::CString;
use std::ptr;

pub struct QuadRenderer {
    program: u32,
    vao: u32,
    vbo: u32,
}

const VERT_SRC: &str = r#"
#version 330 core
layout(location = 0) in vec2 aPos;
layout(location = 1) in vec2 aUV;
out vec2 vUV;
uniform vec4 uRect; // x, y, w, h in NDC
uniform int uFlipY; // 1 = flip vertically (for mpv FBO textures)
void main() {
    vec2 pos = uRect.xy + aPos * uRect.zw;
    gl_Position = vec4(pos, 0.0, 1.0);
    vec2 uv = aUV;
    if (uFlipY != 0) uv.y = 1.0 - uv.y;
    vUV = uv;
}
"#;

const FRAG_SRC: &str = r#"
#version 330 core
in vec2 vUV;
out vec4 fragColor;
uniform sampler2D uTex;
void main() {
    fragColor = texture(uTex, vUV);
}
"#;

impl QuadRenderer {
    pub fn new() -> Self {
        unsafe {
            let program = create_program(VERT_SRC, FRAG_SRC);

            // Unit quad: position (0..1, 0..1) + UV
            #[rustfmt::skip]
            let vertices: [f32; 24] = [
                // pos      uv
                0.0, 0.0,   0.0, 1.0,
                1.0, 0.0,   1.0, 1.0,
                0.0, 1.0,   0.0, 0.0,
                1.0, 0.0,   1.0, 1.0,
                1.0, 1.0,   1.0, 0.0,
                0.0, 1.0,   0.0, 0.0,
            ];

            let mut vao = 0u32;
            let mut vbo = 0u32;
            gl::GenVertexArrays(1, &mut vao);
            gl::GenBuffers(1, &mut vbo);

            gl::BindVertexArray(vao);
            gl::BindBuffer(gl::ARRAY_BUFFER, vbo);
            gl::BufferData(
                gl::ARRAY_BUFFER,
                (vertices.len() * 4) as isize,
                vertices.as_ptr() as *const _,
                gl::STATIC_DRAW,
            );

            // aPos
            gl::EnableVertexAttribArray(0);
            gl::VertexAttribPointer(0, 2, gl::FLOAT, gl::FALSE, 16, ptr::null());
            // aUV
            gl::EnableVertexAttribArray(1);
            gl::VertexAttribPointer(1, 2, gl::FLOAT, gl::FALSE, 16, 8 as *const _);

            gl::BindVertexArray(0);

            QuadRenderer { program, vao, vbo }
        }
    }

    /// Draw a texture fitted within the viewport, preserving aspect ratio.
    /// `flip_y`: set true for mpv video textures (rendered into FBO with GL origin).
    pub fn draw(&self, texture: u32, img_w: u32, img_h: u32, viewport_w: u32, viewport_h: u32) {
        self.draw_inner(texture, img_w, img_h, viewport_w, viewport_h, false);
    }

    /// Draw a video texture (flipped Y to correct for mpv FBO orientation).
    pub fn draw_video(
        &self,
        texture: u32,
        img_w: u32,
        img_h: u32,
        viewport_w: u32,
        viewport_h: u32,
    ) {
        self.draw_inner(texture, img_w, img_h, viewport_w, viewport_h, true);
    }

    fn draw_inner(
        &self,
        texture: u32,
        img_w: u32,
        img_h: u32,
        viewport_w: u32,
        viewport_h: u32,
        flip_y: bool,
    ) {
        let img_aspect = img_w as f32 / img_h.max(1) as f32;
        let vp_aspect = viewport_w as f32 / viewport_h.max(1) as f32;

        // Fit image in viewport
        let (quad_w, quad_h) = if img_aspect > vp_aspect {
            // Image is wider — fit width
            (2.0f32, 2.0 / img_aspect * vp_aspect)
        } else {
            // Image is taller — fit height
            (2.0 * img_aspect / vp_aspect, 2.0f32)
        };

        // Center in NDC (-1..1)
        let x = -quad_w / 2.0;
        let y = -quad_h / 2.0;

        unsafe {
            gl::UseProgram(self.program);

            let loc = gl::GetUniformLocation(self.program, CString::new("uRect").unwrap().as_ptr());
            gl::Uniform4f(loc, x, y, quad_w, quad_h);

            let flip_loc =
                gl::GetUniformLocation(self.program, CString::new("uFlipY").unwrap().as_ptr());
            gl::Uniform1i(flip_loc, flip_y as i32);

            gl::ActiveTexture(gl::TEXTURE0);
            gl::BindTexture(gl::TEXTURE_2D, texture);

            let tex_loc =
                gl::GetUniformLocation(self.program, CString::new("uTex").unwrap().as_ptr());
            gl::Uniform1i(tex_loc, 0);

            gl::BindVertexArray(self.vao);
            gl::DrawArrays(gl::TRIANGLES, 0, 6);
            gl::BindVertexArray(0);
            gl::UseProgram(0);
        }
    }

    /// Draw a texture at an arbitrary NDC rectangle with alpha blending.
    #[allow(dead_code)]
    pub fn draw_rect(&self, texture: u32, x: f32, y: f32, w: f32, h: f32) {
        unsafe {
            gl::Enable(gl::BLEND);
            gl::BlendFunc(gl::SRC_ALPHA, gl::ONE_MINUS_SRC_ALPHA);

            gl::UseProgram(self.program);

            let loc = gl::GetUniformLocation(self.program, CString::new("uRect").unwrap().as_ptr());
            gl::Uniform4f(loc, x, y, w, h);

            gl::ActiveTexture(gl::TEXTURE0);
            gl::BindTexture(gl::TEXTURE_2D, texture);

            let tex_loc =
                gl::GetUniformLocation(self.program, CString::new("uTex").unwrap().as_ptr());
            gl::Uniform1i(tex_loc, 0);

            gl::BindVertexArray(self.vao);
            gl::DrawArrays(gl::TRIANGLES, 0, 6);
            gl::BindVertexArray(0);
            gl::UseProgram(0);

            gl::Disable(gl::BLEND);
        }
    }
}

impl Drop for QuadRenderer {
    fn drop(&mut self) {
        unsafe {
            gl::DeleteProgram(self.program);
            gl::DeleteBuffers(1, &self.vbo);
            gl::DeleteVertexArrays(1, &self.vao);
        }
    }
}

unsafe fn create_program(vert_src: &str, frag_src: &str) -> u32 {
    let vs = compile_shader(gl::VERTEX_SHADER, vert_src);
    let fs = compile_shader(gl::FRAGMENT_SHADER, frag_src);

    let program = gl::CreateProgram();
    gl::AttachShader(program, vs);
    gl::AttachShader(program, fs);
    gl::LinkProgram(program);

    let mut success = 0i32;
    gl::GetProgramiv(program, gl::LINK_STATUS, &mut success);
    if success == 0 {
        let mut len = 0i32;
        gl::GetProgramiv(program, gl::INFO_LOG_LENGTH, &mut len);
        let mut buf = vec![0u8; len as usize];
        gl::GetProgramInfoLog(program, len, ptr::null_mut(), buf.as_mut_ptr() as *mut _);
        panic!("Shader link error: {}", String::from_utf8_lossy(&buf));
    }

    gl::DeleteShader(vs);
    gl::DeleteShader(fs);
    program
}

unsafe fn compile_shader(kind: u32, src: &str) -> u32 {
    let shader = gl::CreateShader(kind);
    let c_src = CString::new(src).unwrap();
    gl::ShaderSource(shader, 1, &c_src.as_ptr(), ptr::null());
    gl::CompileShader(shader);

    let mut success = 0i32;
    gl::GetShaderiv(shader, gl::COMPILE_STATUS, &mut success);
    if success == 0 {
        let mut len = 0i32;
        gl::GetShaderiv(shader, gl::INFO_LOG_LENGTH, &mut len);
        let mut buf = vec![0u8; len as usize];
        gl::GetShaderInfoLog(shader, len, ptr::null_mut(), buf.as_mut_ptr() as *mut _);
        let kind_str = if kind == gl::VERTEX_SHADER {
            "vertex"
        } else {
            "fragment"
        };
        panic!(
            "{} shader compile error: {}",
            kind_str,
            String::from_utf8_lossy(&buf)
        );
    }
    shader
}
