use eframe::{egui, glow, glow::HasContext};
use std::sync::Arc;

// 지도만 별도 GPU 패스로 그린다. egui의 글자와 컨트롤은 기존 렌더러가 담당한다.
pub struct Renderer {
    program: glow::Program,
    vao: glow::VertexArray,
    buffer: glow::Buffer,
}
impl Renderer {
    pub fn new(gl: &glow::Context) -> Result<Self, String> {
        unsafe {
            let program = gl.create_program()?;
            let sources = [
                (
                    glow::VERTEX_SHADER,
                    "#version 330 core\nlayout(location=0) in vec3 position; layout(location=1) in vec4 color; out vec4 tint; void main(){gl_Position=vec4(position,1.0);tint=color;}",
                ),
                (
                    glow::FRAGMENT_SHADER,
                    "#version 330 core\nin vec4 tint; out vec4 out_color; void main(){out_color=tint;}",
                ),
            ];
            for (kind, source) in sources {
                let shader = gl.create_shader(kind)?;
                gl.shader_source(shader, source);
                gl.compile_shader(shader);
                if !gl.get_shader_compile_status(shader) {
                    let error = gl.get_shader_info_log(shader);
                    gl.delete_shader(shader);
                    gl.delete_program(program);
                    return Err(error);
                }
                gl.attach_shader(program, shader);
                gl.delete_shader(shader);
            }
            gl.link_program(program);
            if !gl.get_program_link_status(program) {
                let error = gl.get_program_info_log(program);
                gl.delete_program(program);
                return Err(error);
            }
            let vao = gl.create_vertex_array()?;
            let buffer = gl.create_buffer()?;
            gl.bind_vertex_array(Some(vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(buffer));
            gl.enable_vertex_attrib_array(0);
            gl.vertex_attrib_pointer_f32(0, 3, glow::FLOAT, false, 28, 0);
            gl.enable_vertex_attrib_array(1);
            gl.vertex_attrib_pointer_f32(1, 4, glow::FLOAT, false, 28, 12);
            gl.bind_vertex_array(None);
            Ok(Self {
                program,
                vao,
                buffer,
            })
        }
    }
    pub fn paint(self: &Arc<Self>, painter: &egui::Painter, rect: egui::Rect, vertices: Vec<f32>) {
        let renderer = self.clone();
        painter.add(egui::PaintCallback {
            rect,
            callback: Arc::new(eframe::egui_glow::CallbackFn::new(move |_info, painter| {
                let gl = painter.gl();
                unsafe {
                    gl.enable(glow::DEPTH_TEST);
                    gl.depth_func(glow::LEQUAL);
                    gl.depth_mask(true);
                    gl.clear_depth_f32(1.);
                    gl.clear(glow::DEPTH_BUFFER_BIT);
                    gl.disable(glow::CULL_FACE);
                    gl.enable(glow::BLEND);
                    gl.blend_func(glow::ONE, glow::ONE_MINUS_SRC_ALPHA);
                    gl.use_program(Some(renderer.program));
                    gl.bind_vertex_array(Some(renderer.vao));
                    gl.bind_buffer(glow::ARRAY_BUFFER, Some(renderer.buffer));
                    let bytes = std::slice::from_raw_parts(
                        vertices.as_ptr().cast::<u8>(),
                        vertices.len() * 4,
                    );
                    gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytes, glow::STREAM_DRAW);
                    gl.draw_arrays(glow::TRIANGLES, 0, (vertices.len() / 7) as i32);
                    gl.disable(glow::DEPTH_TEST);
                    gl.depth_mask(false);
                    gl.bind_vertex_array(None);
                }
            })),
        });
    }
    pub fn destroy(&self, gl: &glow::Context) {
        unsafe {
            gl.delete_buffer(self.buffer);
            gl.delete_vertex_array(self.vao);
            gl.delete_program(self.program);
        }
    }
}
