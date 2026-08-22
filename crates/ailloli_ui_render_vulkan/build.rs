//! Build-time GLSL-to-SPIR-V compiler for the Vulkan backend.
//!
//! Every shader is compiled for Vulkan 1.1 and emitted as deterministic Rust
//! `u32` slices in `OUT_DIR/compiled_shaders.rs`. Failures stop the build with
//! the source path and compiler error.

use std::io::Write;
use std::path::Path;

/// Tracks, compiles, and emits every renderer shader consumed at runtime.
fn main() {
    let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let out_path = out_dir.join("compiled_shaders.rs");

    let rect_vert_path = Path::new("src/shaders/solid_rect.vert");
    let rect_frag_path = Path::new("src/shaders/solid_rect.frag");
    let rrect_vert_path = Path::new("src/shaders/rounded_rect.vert");
    let rrect_frag_path = Path::new("src/shaders/rounded_rect.frag");
    let border_vert_path = Path::new("src/shaders/border_rrect.vert");
    let border_frag_path = Path::new("src/shaders/border_rrect.frag");
    let shadow_vert_path = Path::new("src/shaders/box_shadow.vert");
    let shadow_frag_path = Path::new("src/shaders/box_shadow.frag");
    let text_vert_path = Path::new("src/shaders/textured_text.vert");
    let text_frag_path = Path::new("src/shaders/textured_text.frag");
    println!("cargo:rerun-if-changed={}", rect_vert_path.display());
    println!("cargo:rerun-if-changed={}", rect_frag_path.display());
    println!("cargo:rerun-if-changed={}", rrect_vert_path.display());
    println!("cargo:rerun-if-changed={}", rrect_frag_path.display());
    println!("cargo:rerun-if-changed={}", border_vert_path.display());
    println!("cargo:rerun-if-changed={}", border_frag_path.display());
    println!("cargo:rerun-if-changed={}", shadow_vert_path.display());
    println!("cargo:rerun-if-changed={}", shadow_frag_path.display());
    println!("cargo:rerun-if-changed={}", text_vert_path.display());
    println!("cargo:rerun-if-changed={}", text_frag_path.display());

    let rect_vert = compile_glsl(shaderc::ShaderKind::Vertex, rect_vert_path);
    let rect_frag = compile_glsl(shaderc::ShaderKind::Fragment, rect_frag_path);
    let rrect_vert = compile_glsl(shaderc::ShaderKind::Vertex, rrect_vert_path);
    let rrect_frag = compile_glsl(shaderc::ShaderKind::Fragment, rrect_frag_path);
    let border_vert = compile_glsl(shaderc::ShaderKind::Vertex, border_vert_path);
    let border_frag = compile_glsl(shaderc::ShaderKind::Fragment, border_frag_path);
    let shadow_vert = compile_glsl(shaderc::ShaderKind::Vertex, shadow_vert_path);
    let shadow_frag = compile_glsl(shaderc::ShaderKind::Fragment, shadow_frag_path);
    let text_vert = compile_glsl(shaderc::ShaderKind::Vertex, text_vert_path);
    let text_frag = compile_glsl(shaderc::ShaderKind::Fragment, text_frag_path);

    let mut file = std::fs::File::create(out_path).expect("create compiled_shaders.rs");
    write_spv_const(&mut file, "SOLID_RECT_VERT_SPV", &rect_vert);
    write_spv_const(&mut file, "SOLID_RECT_FRAG_SPV", &rect_frag);
    write_spv_const(&mut file, "ROUNDED_RECT_VERT_SPV", &rrect_vert);
    write_spv_const(&mut file, "ROUNDED_RECT_FRAG_SPV", &rrect_frag);
    write_spv_const(&mut file, "BORDER_RRECT_VERT_SPV", &border_vert);
    write_spv_const(&mut file, "BORDER_RRECT_FRAG_SPV", &border_frag);
    write_spv_const(&mut file, "BOX_SHADOW_VERT_SPV", &shadow_vert);
    write_spv_const(&mut file, "BOX_SHADOW_FRAG_SPV", &shadow_frag);
    write_spv_const(&mut file, "TEXTURED_TEXT_VERT_SPV", &text_vert);
    write_spv_const(&mut file, "TEXTURED_TEXT_FRAG_SPV", &text_frag);
}

/// Compiles one UTF-8 GLSL file whose entry point is `main` into SPIR-V words.
///
/// # Panics
///
/// Panics when the source cannot be read, `shaderc` cannot initialize, the path
/// cannot be compiled, or the shader is invalid for the requested stage.
fn compile_glsl(kind: shaderc::ShaderKind, path: &Path) -> Vec<u32> {
    let source = std::fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("read {}: {err}", path.display()));
    let compiler = shaderc::Compiler::new().expect("create shaderc compiler");
    let mut options = shaderc::CompileOptions::new().expect("create shaderc options");
    options.set_target_env(
        shaderc::TargetEnv::Vulkan,
        shaderc::EnvVersion::Vulkan1_1 as u32,
    );
    compiler
        .compile_into_spirv(
            &source,
            kind,
            path.to_str().unwrap_or("shader"),
            "main",
            Some(&options),
        )
        .unwrap_or_else(|err| panic!("compile {}: {err}", path.display()))
        .as_binary()
        .to_vec()
}

/// Writes one public hexadecimal `&[u32]` constant, eight words per source line.
///
/// # Panics
///
/// Panics if any write to the generated Rust file fails.
fn write_spv_const(file: &mut std::fs::File, name: &str, words: &[u32]) {
    writeln!(
        file,
        "/// Build-generated Vulkan 1.1 SPIR-V words for the `{name}` shader."
    )
    .expect("write shader documentation");
    writeln!(file, "///").expect("write shader documentation");
    writeln!(file, "/// # Examples").expect("write shader documentation");
    writeln!(file, "///").expect("write shader documentation");
    writeln!(file, "/// ```").expect("write shader documentation");
    writeln!(file, "/// use ailloli_ui_render_vulkan::shaders::{name};")
        .expect("write shader documentation");
    writeln!(file, "/// assert!(!{name}.is_empty());").expect("write shader documentation");
    writeln!(file, "/// ```").expect("write shader documentation");
    writeln!(file, "pub const {name}: &[u32] = &[").expect("write shader const");
    for chunk in words.chunks(8) {
        write!(file, "    ").expect("write shader indent");
        for word in chunk {
            write!(file, "0x{word:08x}, ").expect("write shader word");
        }
        writeln!(file).expect("write shader newline");
    }
    writeln!(file, "];").expect("write shader const end");
}
