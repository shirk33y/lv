//! AI metadata extraction from PNG (ComfyUI / A1111) files.
//!
//! Extracts prompt and model name from tEXt/iTXt chunks.

use std::io::{Read, Seek, SeekFrom};

/// Extracted AI generation parameters.
pub struct AiBasic {
    pub prompt: String,
    pub model: String,
}

/// Read PNG tEXt/iTXt chunks and extract AI metadata.
pub fn extract_png(path: &str) -> Result<AiBasic, String> {
    let mut f = std::fs::File::open(path).map_err(|e| e.to_string())?;

    // Verify PNG signature
    let mut sig = [0u8; 8];
    f.read_exact(&mut sig).map_err(|e| e.to_string())?;
    if sig != [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A] {
        return Err("not a PNG".into());
    }

    let mut text_chunks: Vec<(String, String)> = Vec::new();

    loop {
        let mut len_buf = [0u8; 4];
        if f.read_exact(&mut len_buf).is_err() {
            break;
        }
        let len = u32::from_be_bytes(len_buf) as usize;

        let mut type_buf = [0u8; 4];
        if f.read_exact(&mut type_buf).is_err() {
            break;
        }
        let chunk_type = std::str::from_utf8(&type_buf).unwrap_or("");

        match chunk_type {
            "tEXt" => {
                let mut data = vec![0u8; len];
                f.read_exact(&mut data).map_err(|e| e.to_string())?;
                if let Some(null) = data.iter().position(|&b| b == 0) {
                    let key = String::from_utf8_lossy(&data[..null]).to_string();
                    let val = String::from_utf8_lossy(&data[null + 1..]).to_string();
                    text_chunks.push((key, val));
                }
            }
            "iTXt" => {
                let mut data = vec![0u8; len];
                f.read_exact(&mut data).map_err(|e| e.to_string())?;
                if let Some(null) = data.iter().position(|&b| b == 0) {
                    let key = String::from_utf8_lossy(&data[..null]).to_string();
                    let rest = &data[null + 1..];
                    if rest.len() >= 2 {
                        let comp_flag = rest[0];
                        let after = &rest[2..]; // skip comp flag + method
                                                // skip language\0translated_keyword\0
                        let mut pos = 0;
                        let mut nulls = 0;
                        for (i, &b) in after.iter().enumerate() {
                            if b == 0 {
                                nulls += 1;
                                if nulls >= 2 {
                                    pos = i + 1;
                                    break;
                                }
                            }
                        }
                        let text_data = &after[pos..];
                        let val = if comp_flag == 1 {
                            decompress(text_data)
                        } else {
                            String::from_utf8_lossy(text_data).to_string()
                        };
                        text_chunks.push((key, val));
                    }
                }
            }
            "IEND" => {
                f.seek(SeekFrom::Current(4)).ok(); // CRC
                break;
            }
            _ => {
                f.seek(SeekFrom::Current(len as i64)).ok();
            }
        }
        f.seek(SeekFrom::Current(4)).ok(); // CRC
    }

    // Try ComfyUI format first (tEXt key "prompt" with JSON)
    if let Some((_, json)) = text_chunks.iter().find(|(k, _)| k == "prompt") {
        if json.starts_with('{') {
            if let Some(ai) = parse_comfyui(json) {
                return Ok(ai);
            }
        }
    }

    // Try A1111 format (tEXt key "parameters")
    if let Some((_, params)) = text_chunks.iter().find(|(k, _)| k == "parameters") {
        return Ok(parse_a1111(params));
    }

    Err("no AI metadata found".into())
}

fn decompress(data: &[u8]) -> String {
    use flate2::read::ZlibDecoder;
    let mut decoder = ZlibDecoder::new(data);
    let mut out = String::new();
    decoder.read_to_string(&mut out).ok();
    out
}

/// Parse ComfyUI workflow JSON → extract prompt + model.
fn parse_comfyui(json: &str) -> Option<AiBasic> {
    let root: serde_json::Value = serde_json::from_str(json).ok()?;
    let obj = root.as_object()?;

    let mut prompt = String::new();
    let mut model = String::new();

    for (_id, node) in obj {
        let class = node["class_type"].as_str().unwrap_or("");
        let inputs = &node["inputs"];
        let title = node["_meta"]["title"].as_str().unwrap_or("");

        match class {
            "CLIPTextEncode" => {
                if let Some(text) = inputs["text"].as_str() {
                    let is_neg = title.to_lowercase().contains("negative");
                    if !is_neg && (prompt.is_empty() || title.to_lowercase().contains("positive")) {
                        prompt = text.to_string();
                    }
                }
            }
            "UNETLoader" | "CheckpointLoaderSimple" | "CheckpointLoader" => {
                let name = inputs["unet_name"]
                    .as_str()
                    .or_else(|| inputs["ckpt_name"].as_str());
                if let Some(n) = name {
                    model = n.to_string();
                }
            }
            _ => {}
        }
    }

    if prompt.is_empty() && model.is_empty() {
        return None;
    }
    Some(AiBasic { prompt, model })
}

/// Parse A1111 parameters text → extract prompt + model.
fn parse_a1111(params: &str) -> AiBasic {
    // Format: prompt\nNegative prompt: ...\nSteps: N, ..., Model: name, ...
    let mut model = String::new();

    let mut lines = params.lines();

    // First line(s) until "Negative prompt:" or key-value line
    let mut prompt_lines = Vec::new();
    for line in &mut lines {
        if line.starts_with("Negative prompt:") || (line.contains(": ") && line.contains(", ")) {
            if line.contains("Model:") || line.contains("Steps:") {
                for pair in line.split(", ") {
                    if let Some((k, v)) = pair.split_once(": ") {
                        if k == "Model" {
                            model = v.to_string();
                        }
                    }
                }
            }
            break;
        }
        prompt_lines.push(line);
    }
    let prompt = prompt_lines.join("\n");

    // Continue scanning remaining lines for Model
    for line in lines {
        if line.contains("Model:") || line.contains("Steps:") {
            for pair in line.split(", ") {
                if let Some((k, v)) = pair.split_once(": ") {
                    if k == "Model" {
                        model = v.to_string();
                    }
                }
            }
        }
    }

    AiBasic { prompt, model }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_comfyui_json() {
        let json = r#"{"3":{"inputs":{"seed":123,"steps":9,"cfg":1.0,"sampler_name":"euler","model":["16",0],"positive":["6",0],"negative":["7",0],"latent_image":["13",0]},"class_type":"KSampler","_meta":{"title":"KSampler"}},"6":{"inputs":{"text":"a cute cat","clip":["18",0]},"class_type":"CLIPTextEncode","_meta":{"title":"CLIP Text Encode (Positive Prompt)"}},"7":{"inputs":{"text":"ugly","clip":["18",0]},"class_type":"CLIPTextEncode","_meta":{"title":"CLIP Text Encode (Negative Prompt)"}},"16":{"inputs":{"unet_name":"model.safetensors"},"class_type":"UNETLoader","_meta":{"title":"Load Diffusion Model"}}}"#;

        let ai = parse_comfyui(json).unwrap();
        assert_eq!(ai.prompt, "a cute cat");
        assert_eq!(ai.model, "model.safetensors");
    }

    #[test]
    fn parse_a1111_text() {
        let params = "a beautiful landscape\nNegative prompt: ugly\nSteps: 20, Sampler: Euler a, CFG scale: 7, Seed: 42, Model: sd_xl_base";
        let ai = parse_a1111(params);
        assert_eq!(ai.prompt, "a beautiful landscape");
        assert_eq!(ai.model, "sd_xl_base");
    }

    fn read_fixture(name: &str) -> String {
        let path = format!("{}/test/fixtures/{}", env!("CARGO_MANIFEST_DIR"), name);
        std::fs::read_to_string(&path).expect("fixture not found")
    }

    #[test]
    fn workflow_basic_t2i() {
        let json = read_fixture("workflow_basic_t2i.json");
        let ai = parse_comfyui(&json).expect("should parse basic t2i");
        assert_eq!(
            ai.prompt,
            "beautiful scenery nature glass bottle landscape, purple galaxy bottle,"
        );
        assert_eq!(ai.model, "v1-5-pruned-emaonly-fp16.safetensors");
    }

    #[test]
    fn workflow_flux_t2i() {
        let json = read_fixture("workflow_flux_t2i.json");
        let ai = parse_comfyui(&json).expect("should parse flux t2i");
        assert!(ai.prompt.contains("anime"));
        assert_eq!(ai.model, "flux_dev_bf16.safetensors");
    }

    #[test]
    fn workflow_prompt_only_no_model() {
        let json = read_fixture("workflow_prompt_only.json");
        let ai = parse_comfyui(&json).expect("should parse even without model");
        assert_eq!(ai.prompt, "a cute cat");
        assert!(ai.model.is_empty());
    }

    #[test]
    fn workflow_z_image_turbo() {
        let json = read_fixture("workflow_z_image_turbo.json");
        let ai = parse_comfyui(&json).expect("should parse z-image-turbo workflow");
        assert!(ai.prompt.contains("anime"));
        assert!(ai.model.contains("z_image_turbo"));
        assert!(ai.model.contains("safetensors"));
    }

    #[test]
    fn workflow_invalid_json() {
        let json = read_fixture("workflow_invalid.json");
        assert!(parse_comfyui(&json).is_none());
    }

    #[test]
    fn parse_comfyui_empty_object() {
        assert!(parse_comfyui("{}").is_none());
    }

    #[test]
    fn parse_comfyui_no_recognized_nodes() {
        let json =
            r#"{"1":{"inputs":{"seed":123},"class_type":"KSampler","_meta":{"title":"KSampler"}}}"#;
        assert!(parse_comfyui(json).is_none());
    }

    #[test]
    fn parse_comfyui_non_string_text_skips_node() {
        let json = r#"{"6":{"inputs":{"text":123},"class_type":"CLIPTextEncode","_meta":{"title":"CLIP Text Encode"}},"16":{"inputs":{"unet_name":"model.safetensors"},"class_type":"UNETLoader"}}"#;
        let ai = parse_comfyui(json).unwrap();
        assert!(ai.prompt.is_empty(), "non-string text should be skipped");
        assert_eq!(ai.model, "model.safetensors");
    }

    #[test]
    fn parse_comfyui_ckpt_name() {
        let json = r#"{"6":{"inputs":{"text":"hello"},"class_type":"CLIPTextEncode","_meta":{"title":"CLIP Text Encode (Positive)"}},"3":{"inputs":{"ckpt_name":"sd_xl.safetensors"},"class_type":"CheckpointLoaderSimple"}}"#;
        let ai = parse_comfyui(json).unwrap();
        assert_eq!(ai.prompt, "hello");
        assert_eq!(ai.model, "sd_xl.safetensors");
    }

    #[test]
    fn parse_comfyui_multiple_positives_uses_first() {
        let json = r#"{"5":{"inputs":{"text":"first prompt"},"class_type":"CLIPTextEncode","_meta":{"title":"CLIP Text Encode (Positive)"}},"10":{"inputs":{"text":"second prompt"},"class_type":"CLIPTextEncode","_meta":{"title":"CLIP Text Encode"}},"16":{"inputs":{"unet_name":"m.safetensors"},"class_type":"UNETLoader"}}"#;
        let ai = parse_comfyui(json).unwrap();
        assert_eq!(ai.prompt, "first prompt");
    }

    #[test]
    fn parse_comfyui_multiple_positives_has_positive_title_second() {
        let json = r#"{"5":{"inputs":{"text":"first prompt"},"class_type":"CLIPTextEncode","_meta":{"title":"CLIP Text Encode"}},"10":{"inputs":{"text":"second prompt"},"class_type":"CLIPTextEncode","_meta":{"title":"Positive Prompt"}},"16":{"inputs":{"unet_name":"m.safetensors"},"class_type":"UNETLoader"}}"#;
        let ai = parse_comfyui(json).unwrap();
        assert_eq!(ai.prompt, "second prompt");
    }

    #[test]
    fn parse_comfyui_empty_model_name() {
        let json = r#"{"6":{"inputs":{"text":"hi"},"class_type":"CLIPTextEncode"},"16":{"inputs":{"unet_name":""},"class_type":"UNETLoader"}}"#;
        let ai = parse_comfyui(json).unwrap();
        assert_eq!(ai.prompt, "hi");
        assert_eq!(ai.model, "");
    }

    #[test]
    fn parse_a1111_no_model() {
        let params =
            "a scenic view\nNegative prompt: clutter\nSteps: 20, Sampler: Euler a, CFG scale: 7";
        let ai = parse_a1111(params);
        assert_eq!(ai.prompt, "a scenic view");
        assert_eq!(ai.model, "");
    }

    #[test]
    fn parse_a1111_no_negative() {
        let params = "just a cat\nSteps: 10, Sampler: DPM++ 2M, Model: catgen_v2";
        let ai = parse_a1111(params);
        assert_eq!(ai.prompt, "just a cat");
        assert_eq!(ai.model, "catgen_v2");
    }

    #[test]
    fn parse_a1111_multiline_prompt() {
        let params =
            "line one\nline two\nline three\nNegative prompt: bad\nSteps: 20, Model: m.safetensors";
        let ai = parse_a1111(params);
        assert_eq!(ai.prompt, "line one\nline two\nline three");
    }

    #[test]
    fn parse_a1111_model_only_params_line() {
        let params = "prompt text\nNegative prompt: bad\nSteps: 20, Sampler: Euler, Model: my_model.safetensors, Seed: 42";
        let ai = parse_a1111(params);
        assert_eq!(ai.prompt, "prompt text");
        assert_eq!(ai.model, "my_model.safetensors");
    }

    #[test]
    fn extract_png_not_a_png() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fake.txt");
        std::fs::write(&path, b"not a png").unwrap();
        let result = extract_png(&path.to_string_lossy());
        assert!(result.is_err());
    }

    #[test]
    fn extract_png_nonexistent_file() {
        let result = extract_png("/nonexistent/test.png");
        assert!(result.is_err());
    }

    #[test]
    fn extract_test_png() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/test/z_image_turbo_example.png"
        );
        if std::path::Path::new(path).exists() {
            let ai = extract_png(path).unwrap();
            assert!(ai.prompt.contains("anime"));
            assert!(ai.model.contains("z_image_turbo"));
        }
    }
}
