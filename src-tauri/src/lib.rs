use csv::{ReaderBuilder, WriterBuilder};
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::{CompressionType, FilterType as PngFilter, PngEncoder};
use image::imageops::FilterType;
use image::{ImageEncoder, ImageFormat};
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::time::Duration;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Manager};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

static SHORTCUT_PRESET: AtomicU8 = AtomicU8::new(0);
static SHORTCUT_ENABLED: AtomicBool = AtomicBool::new(true);
static AUTO_GRAB_FILE: AtomicBool = AtomicBool::new(true);

#[cfg(target_os = "windows")]
#[link(name = "user32")]
unsafe extern "system" {
    fn GetAsyncKeyState(vKey: i32) -> i16;
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MorphResult {
    pub output_path: String,
    pub output_paths: Vec<String>,
    pub old_size: u64,
    pub new_size: u64,
    pub files_count: usize,
}

// ============================================================================
// VALIDAZIONE PREVENTIVA PER BATCH OMOGENEI ED ETEROGENEI
// ============================================================================
fn is_conversion_supported(input_ext: &str, target_format: &str, is_batch: bool) -> bool {
    let ext = if input_ext == "jpeg" { "jpg" } else { input_ext };
    if target_format == "zip" {
        return is_batch || ext != "zip";
    }
    if target_format == "unzip" {
        return ext == "zip";
    }
    if ext == target_format {
        return false;
    }

    let is_img = ["png", "jpg", "webp", "bmp", "gif", "ico", "tiff"].contains(&ext);
    let is_doc = ["pdf", "docx", "txt", "md", "html"].contains(&ext);
    let is_data = ["csv", "json"].contains(&ext);

    if is_img {
        return ["webp", "png", "jpg", "pdf", "ico", "compress"].contains(&target_format);
    }
    if is_doc {
        return ["pdf", "docx", "txt", "md", "html"].contains(&target_format);
    }
    if is_data {
        return ["json", "csv", "html", "md", "pdf", "txt"].contains(&target_format);
    }
    false
}

fn resolve_target_dir(input_path: &Path, output_mode: &str) -> PathBuf {
    let default_parent = input_path.parent().unwrap_or(Path::new(".")).to_path_buf();
    let user_profile = std::env::var("USERPROFILE").ok().map(PathBuf::from);

    match output_mode {
        "desktop" => user_profile
            .map(|p| p.join("Desktop"))
            .filter(|p| p.exists())
            .unwrap_or(default_parent),
        "downloads" => user_profile
            .map(|p| p.join("Downloads"))
            .filter(|p| p.exists())
            .unwrap_or(default_parent),
        _ => default_parent,
    }
}

fn get_unique_output_path(
    input_path: &Path,
    target_dir: &Path,
    stem: &str,
    ext: &str,
    force_suffix: bool,
) -> PathBuf {
    let mut candidate = target_dir.join(format!("{}.{}", stem, ext));
    if candidate == input_path || force_suffix {
        candidate = target_dir.join(format!("{}_morph.{}", stem, ext));
    }
    let mut counter = 2;
    while candidate.exists() {
        candidate = target_dir.join(format!("{}_morph_{}.{}", stem, counter, ext));
        counter += 1;
    }
    candidate
}

// ============================================================================
// CATTURA MULTI-FILE SELEZIONATI IN ESPLORA FILE DI WINDOWS
// ============================================================================
#[cfg(target_os = "windows")]
fn get_selected_explorer_files() -> Vec<String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;

    let script = r#"
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$code = '[DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();'
$fg = (Add-Type -MemberDefinition $code -Name W -Namespace T -PassThru)::GetForegroundWindow().ToInt64()
$sh = New-Object -ComObject Shell.Application
$wins = @($sh.Windows())
foreach ($w in $wins) {
    try {
        if ($w.HWND -eq $fg -and $w.Document.SelectedItems().Count -gt 0) {
            foreach ($item in $w.Document.SelectedItems()) { Write-Output $item.Path }
            exit
        }
    } catch {}
}
foreach ($w in $wins) {
    try {
        if ($w.Document.SelectedItems().Count -gt 0) {
            foreach ($item in $w.Document.SelectedItems()) { Write-Output $item.Path }
            exit
        }
    } catch {}
}
"#;

    let Ok(output) = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
    else {
        return Vec::new();
    };

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|p| !p.is_empty() && Path::new(p).is_file())
        .collect()
}

#[cfg(not(target_os = "windows"))]
fn get_selected_explorer_files() -> Vec<String> {
    Vec::new()
}

// ============================================================================
// COSTRUTTORE PDF 1.4 CON SUPPORTO WINANSI (ACCENTI ITALIANI & TITOLI)
// ============================================================================
fn build_pdf_from_objects(objects: Vec<Vec<u8>>) -> Vec<u8> {
    let mut pdf = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();

    for (i, obj) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        pdf.extend_from_slice(obj);
        pdf.extend_from_slice(b"\nendobj\n");
    }

    let xref_offset = pdf.len();
    pdf.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets {
        pdf.extend_from_slice(format!("{:010} 00000 n \n", offset).as_bytes());
    }

    pdf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF",
            objects.len() + 1,
            xref_offset
        )
        .as_bytes(),
    );
    pdf
}

fn encode_pdf_winansi(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '(' => out.push_str("\\("),
            ')' => out.push_str("\\)"),
            '€' => out.push_str("\\200"),
            '•' => out.push_str("\\225"),
            '–' | '—' => out.push('-'),
            '“' | '”' => out.push('"'),
            '’' | '‘' => out.push('\''),
            c if (c as u32) >= 32 && (c as u32) <= 126 => out.push(c),
            c if (c as u32) >= 160 && (c as u32) <= 255 => {
                out.push_str(&format!("\\{:03o}", c as u32));
            }
            _ => out.push('?'),
        }
    }
    out
}

fn save_image_as_pdf(img: &image::DynamicImage, output_path: &Path) -> Result<(), String> {
    let rgb = img.to_rgb8();
    let (w, h) = (rgb.width(), rgb.height());
    let mut jpeg_bytes = Vec::new();
    let mut encoder = JpegEncoder::new_with_quality(&mut jpeg_bytes, 88);
    encoder.encode_image(&rgb).map_err(|e| e.to_string())?;

    let content = format!("q\n{} 0 0 {} 0 0 cm\n/Im0 Do\nQ", w, h);

    let obj1 = b"<< /Type /Catalog /Pages 2 0 R >>".to_vec();
    let obj2 = b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec();
    let obj3 = format!(
        "<< /Type /Page /Parent 2 0 R /Resources << /XObject << /Im0 4 0 R >> >> /MediaBox [0 0 {} {}] /Contents 5 0 R >>",
        w, h
    ).into_bytes();

    let mut obj4 = format!(
        "<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode /Length {} >>\nstream\n",
        w, h, jpeg_bytes.len()
    ).into_bytes();
    obj4.extend_from_slice(&jpeg_bytes);
    obj4.extend_from_slice(b"\nendstream");

    let obj5 = format!(
        "<< /Length {} >>\nstream\n{}\nendstream",
        content.len(),
        content
    )
    .into_bytes();

    let pdf_bytes = build_pdf_from_objects(vec![obj1, obj2, obj3, obj4, obj5]);
    fs::write(output_path, pdf_bytes).map_err(|e| e.to_string())
}

fn save_text_as_pdf(text: &str, output_path: &Path) -> Result<(), String> {
    let mut wrapped_lines: Vec<(bool, String)> = Vec::new();
    for raw_line in text.lines() {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() {
            wrapped_lines.push((false, String::new()));
            continue;
        }
        let is_heading = trimmed.starts_with('#');
        let clean_line = if is_heading {
            trimmed.trim_start_matches('#').trim()
        } else {
            raw_line
        };

        let mut current = String::new();
        for word in clean_line.split_whitespace() {
            if current.len() + word.len() + 1 > 82 {
                wrapped_lines.push((is_heading, current));
                current = word.to_string();
            } else {
                if !current.is_empty() {
                    current.push(' ');
                }
                current.push_str(word);
            }
        }
        if !current.is_empty() {
            wrapped_lines.push((is_heading, current));
        }
    }
    if wrapped_lines.is_empty() {
        wrapped_lines.push((false, "(Documento vuoto)".to_string()));
    }

    let lines_per_page = 46;
    let chunks: Vec<&[(bool, String)]> = wrapped_lines.chunks(lines_per_page).collect();
    let num_pages = chunks.len();

    let mut objects: Vec<Vec<u8>> = Vec::new();
    objects.push(b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()); // 1
    objects.push(Vec::new()); // 2: Pages
    objects.push(
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
            .to_vec(),
    ); // 3: Regular Font
    objects.push(
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>"
            .to_vec(),
    ); // 4: Bold Font

    let mut kids_refs = Vec::new();

    for chunk in chunks {
        let page_obj_id = objects.len() + 1;
        let content_obj_id = page_obj_id + 1;
        kids_refs.push(format!("{} 0 R", page_obj_id));

        let mut stream = String::from("BT\n15 TL\n50 785 Td\n");
        for (is_heading, line) in chunk {
            let escaped = encode_pdf_winansi(line);
            if *is_heading {
                stream.push_str(&format!("/F2 13 Tf\n({}) '\n", escaped));
            } else {
                stream.push_str(&format!("/F1 11 Tf\n({}) '\n", escaped));
            }
        }
        stream.push_str("ET");

        let page_obj = format!(
            "<< /Type /Page /Parent 2 0 R /Resources << /Font << /F1 3 0 R /F2 4 0 R >> >> /MediaBox [0 0 595 842] /Contents {} 0 R >>",
            content_obj_id
        ).into_bytes();
        let content_obj = format!(
            "<< /Length {} >>\nstream\n{}\nendstream",
            stream.len(),
            stream
        )
        .into_bytes();

        objects.push(page_obj);
        objects.push(content_obj);
    }

    objects[1] = format!(
        "<< /Type /Pages /Kids [{}] /Count {} >>",
        kids_refs.join(" "),
        num_pages
    )
    .into_bytes();

    let pdf_bytes = build_pdf_from_objects(objects);
    fs::write(output_path, pdf_bytes).map_err(|e| e.to_string())
}

// ============================================================================
// MOTORE DOCUMENTI WORD (.DOCX CON TABELLE ED ELENCHI) E PDF
// ============================================================================
fn extract_text_from_docx(path: &Path) -> Result<String, String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    let mut archive = ZipArchive::new(file).map_err(|_| "File DOCX non valido".to_string())?;
    let mut doc_xml = String::new();
    archive
        .by_name("word/document.xml")
        .map_err(|_| "Struttura Word non trovata".to_string())?
        .read_to_string(&mut doc_xml)
        .map_err(|e| e.to_string())?;

    let normalized_xml = doc_xml
        .replace("<w:br/>", "\n")
        .replace("<w:br />", "\n")
        .replace("<w:tab/>", "\t")
        .replace("</w:tc>", " | ")
        .replace("</w:tr>", "\n");

    let mut out = String::new();
    for para in normalized_xml.split("</w:p>") {
        let is_list_item = para.contains("<w:numPr>");
        let is_heading = para.contains("Heading") || para.contains("Titolo");
        let mut p_text = String::new();

        for part in para.split("<w:t") {
            if let Some(gt_idx) = part.find('>') {
                let after_gt = &part[gt_idx + 1..];
                if let Some(end_idx) = after_gt.find("</w:t>") {
                    p_text.push_str(&after_gt[..end_idx]);
                }
            }
        }
        if !p_text.trim().is_empty() {
            let decoded = p_text
                .replace("&amp;", "&")
                .replace("&lt;", "<")
                .replace("&gt;", ">")
                .replace("&quot;", "\"")
                .replace("&apos;", "'");
            if is_heading {
                out.push_str("# ");
            } else if is_list_item {
                out.push_str("• ");
            }
            out.push_str(decoded.trim());
            out.push('\n');
        }
    }
    Ok(out.trim().to_string())
}

fn create_docx_from_text(text: &str, output_path: &Path) -> Result<(), String> {
    let file = File::create(output_path).map_err(|e| e.to_string())?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    let content_types = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
</Types>"#;

    let rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#;

    let mut body_xml = String::new();
    for line in text.lines() {
        let trimmed = line.trim_start_matches('#').trim();
        let is_heading = line.trim_start().starts_with('#');
        let escaped = trimmed
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;");

        if is_heading {
            body_xml.push_str(&format!(
                "<w:p><w:r><w:rPr><w:b/><w:sz w:val=\"28\"/></w:rPr><w:t xml:space=\"preserve\">{}</w:t></w:r></w:p>",
                escaped
            ));
        } else {
            body_xml.push_str(&format!(
                "<w:p><w:r><w:t xml:space=\"preserve\">{}</w:t></w:r></w:p>",
                escaped
            ));
        }
    }

    let document_xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>{}</w:body>
</w:document>"#,
        body_xml
    );

    zip.start_file("[Content_Types].xml", options)
        .map_err(|e| e.to_string())?;
    zip.write_all(content_types.as_bytes())
        .map_err(|e| e.to_string())?;
    zip.start_file("_rels/.rels", options)
        .map_err(|e| e.to_string())?;
    zip.write_all(rels.as_bytes())
        .map_err(|e| e.to_string())?;
    zip.start_file("word/document.xml", options)
        .map_err(|e| e.to_string())?;
    zip.write_all(document_xml.as_bytes())
        .map_err(|e| e.to_string())?;
    zip.finish().map_err(|e| e.to_string())?;
    Ok(())
}

fn extract_text_from_pdf(path: &Path) -> Result<String, String> {
    let doc = lopdf::Document::load(path).map_err(|e| format!("PDF non leggibile: {}", e))?;
    let pages = doc.get_pages();
    let mut page_nums: Vec<u32> = pages.keys().cloned().collect();
    page_nums.sort();
    let extracted = doc.extract_text(&page_nums).unwrap_or_default();
    if extracted.trim().is_empty() {
        Ok("[Nota: questo PDF contiene scansioni fotografiche senza testo selezionabile]".into())
    } else {
        Ok(extracted.trim().to_string())
    }
}

// ============================================================================
// MOTORE DATI ROBUSTO RFC-4180 (CSV ⇄ JSON CON OGGETTI ANNIDATI)
// ============================================================================
fn detect_csv_delimiter(csv_text: &str) -> u8 {
    let first_line = csv_text.lines().next().unwrap_or("");
    let semicolons = first_line.matches(';').count();
    let commas = first_line.matches(',').count();
    let tabs = first_line.matches('\t').count();
    if tabs > commas && tabs > semicolons {
        b'\t'
    } else if semicolons > commas {
        b';'
    } else {
        b','
    }
}

fn convert_csv_rfc4180(csv_text: &str, target: &str) -> Result<String, String> {
    let delimiter = detect_csv_delimiter(csv_text);
    let mut rdr = ReaderBuilder::new()
        .delimiter(delimiter)
        .flexible(true)
        .from_reader(csv_text.as_bytes());

    let headers: Vec<String> = rdr
        .headers()
        .map_err(|e| format!("Intestazione CSV non valida: {}", e))?
        .iter()
        .map(|h| h.trim().to_string())
        .collect();

    if headers.is_empty() {
        return Ok(String::new());
    }

    let mut rows: Vec<Vec<String>> = Vec::new();
    for result in rdr.records() {
        let record = result.map_err(|e| format!("Riga CSV malformata: {}", e))?;
        rows.push(record.iter().map(|c| c.to_string()).collect());
    }

    match target {
        "json" => {
            let mut arr = Vec::new();
            for row in rows {
                let mut map = serde_json::Map::new();
                for (i, h) in headers.iter().enumerate() {
                    let raw_val = row.get(i).cloned().unwrap_or_default();
                    let json_val = if let Ok(n) = raw_val.parse::<i64>() {
                        serde_json::Value::Number(n.into())
                    } else if let Ok(f) = raw_val.parse::<f64>() {
                        serde_json::Number::from_f64(f)
                            .map(serde_json::Value::Number)
                            .unwrap_or(serde_json::Value::String(raw_val))
                    } else if raw_val.eq_ignore_ascii_case("true") {
                        serde_json::Value::Bool(true)
                    } else if raw_val.eq_ignore_ascii_case("false") {
                        serde_json::Value::Bool(false)
                    } else {
                        serde_json::Value::String(raw_val)
                    };
                    map.insert(h.clone(), json_val);
                }
                arr.push(serde_json::Value::Object(map));
            }
            serde_json::to_string_pretty(&arr).map_err(|e| e.to_string())
        }
        "md" => {
            let mut md = format!(
                "| {} |\n| {} |\n",
                headers.join(" | "),
                headers
                    .iter()
                    .map(|_| "---")
                    .collect::<Vec<_>>()
                    .join(" | ")
            );
            for row in rows {
                let escaped_row: Vec<String> = row
                    .iter()
                    .map(|c| c.replace('|', "\\|").replace('\n', " "))
                    .collect();
                md.push_str(&format!("| {} |\n", escaped_row.join(" | ")));
            }
            Ok(md)
        }
        "html" => {
            let ths: String = headers.iter().map(|h| format!("<th>{}</th>", h)).collect();
            let mut trs = String::new();
            for row in rows {
                let tds: String = row
                    .iter()
                    .map(|c| {
                        format!(
                            "<td>{}</td>",
                            c.replace('&', "&amp;").replace('<', "&lt;")
                        )
                    })
                    .collect();
                trs.push_str(&format!("<tr>{}</tr>\n", tds));
            }
            Ok(format!(
                "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><style>body{{font-family:system-ui,sans-serif;padding:24px}}table{{border-collapse:collapse;width:100%}}th,td{{border:1px solid #e4e4e7;padding:8px 12px;text-align:left}}th{{background:#f4f4f5;font-weight:600}}</style></head><body><table><thead><tr>{}</tr></thead><tbody>{}</tbody></table></body></html>",
                ths, trs
            ))
        }
        _ => Ok(csv_text.to_string()),
    }
}

fn flatten_json_value(
    prefix: &str,
    val: &serde_json::Value,
    out: &mut BTreeMap<String, String>,
) {
    match val {
        serde_json::Value::Object(map) => {
            for (k, v) in map {
                let new_key = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{}.{}", prefix, k)
                };
                flatten_json_value(&new_key, v, out);
            }
        }
        serde_json::Value::Array(arr) => {
            let serialized = serde_json::to_string(arr).unwrap_or_default();
            out.insert(prefix.to_string(), serialized);
        }
        serde_json::Value::String(s) => {
            out.insert(prefix.to_string(), s.clone());
        }
        serde_json::Value::Null => {
            out.insert(prefix.to_string(), String::new());
        }
        other => {
            out.insert(prefix.to_string(), other.to_string());
        }
    }
}

fn convert_json_to_csv_rfc4180(json_text: &str) -> Result<String, String> {
    let val: serde_json::Value =
        serde_json::from_str(json_text).map_err(|e| format!("JSON non valido: {}", e))?;

    let items: Vec<serde_json::Value> = match val {
        serde_json::Value::Array(arr) => arr,
        serde_json::Value::Object(_) => vec![val],
        _ => return Err("Il JSON deve contenere un oggetto o una lista di oggetti".into()),
    };

    let mut flattened_rows: Vec<BTreeMap<String, String>> = Vec::new();
    let mut all_keys: Vec<String> = Vec::new();

    for item in &items {
        let mut row_map = BTreeMap::new();
        flatten_json_value("", item, &mut row_map);
        for k in row_map.keys() {
            if !all_keys.contains(k) {
                all_keys.push(k.clone());
            }
        }
        flattened_rows.push(row_map);
    }

    let mut wtr = WriterBuilder::new().from_writer(vec![]);
    wtr.write_record(&all_keys).map_err(|e| e.to_string())?;

    for row_map in flattened_rows {
        let record: Vec<String> = all_keys
            .iter()
            .map(|k| row_map.get(k).cloned().unwrap_or_default())
            .collect();
        wtr.write_record(&record).map_err(|e| e.to_string())?;
    }

    let data = wtr.into_inner().map_err(|e| e.to_string())?;
    String::from_utf8(data).map_err(|e| e.to_string())
}

// ============================================================================
// ARCHIVI ZIP E CONVERSIONE ATOMICA CON ROLLBACK
// ============================================================================
fn create_zip_from_files(input_paths: &[PathBuf], output_path: &Path) -> Result<(), String> {
    let file = File::create(output_path).map_err(|e| e.to_string())?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    for path in input_paths {
        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
        zip.start_file(file_name, options)
            .map_err(|e| e.to_string())?;
        let bytes = fs::read(path).map_err(|e| e.to_string())?;
        zip.write_all(&bytes).map_err(|e| e.to_string())?;
    }
    zip.finish().map_err(|e| e.to_string())?;
    Ok(())
}

fn extract_zip_archive(input_path: &Path, dest_dir: &Path) -> Result<u64, String> {
    let file = File::open(input_path).map_err(|e| e.to_string())?;
    let mut archive = ZipArchive::new(file).map_err(|e| e.to_string())?;
    fs::create_dir_all(dest_dir).map_err(|e| e.to_string())?;

    let mut total_bytes = 0u64;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let outpath = match entry.enclosed_name() {
            Some(path) => dest_dir.join(path),
            None => continue,
        };
        if entry.is_dir() {
            fs::create_dir_all(&outpath).map_err(|e| e.to_string())?;
        } else {
            if let Some(p) = outpath.parent() {
                fs::create_dir_all(p).map_err(|e| e.to_string())?;
            }
            let mut outfile = File::create(&outpath).map_err(|e| e.to_string())?;
            total_bytes += std::io::copy(&mut entry, &mut outfile).map_err(|e| e.to_string())?;
        }
    }
    Ok(total_bytes)
}

fn convert_single_file(
    input_path: &Path,
    target_format: &str,
    output_mode: &str,
) -> Result<(PathBuf, u64, u64), String> {
    if !input_path.exists() {
        return Err(format!("File non trovato: {}", input_path.display()));
    }

    let old_size = fs::metadata(input_path).map_err(|e| e.to_string())?.len();
    let stem = input_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("output");
    let target_dir = resolve_target_dir(input_path, output_mode);
    let input_ext = input_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    if target_format == "unzip" {
        let extract_dir = target_dir.join(format!("{}_extracted", stem));
        let new_size = extract_zip_archive(input_path, &extract_dir)?;
        return Ok((extract_dir, old_size, new_size));
    }

    let ext = match target_format {
        "compress" => "jpg",
        other => other,
    };

    let output_path = get_unique_output_path(
        input_path,
        &target_dir,
        stem,
        ext,
        target_format == "compress",
    );

    if target_format == "zip" {
        create_zip_from_files(&[input_path.to_path_buf()], &output_path)?;
        let new_size = fs::metadata(&output_path).map_err(|e| e.to_string())?.len();
        return Ok((output_path, old_size, new_size));
    }

    let is_image_input = ["png", "jpg", "jpeg", "webp", "bmp", "gif", "ico", "tiff"]
        .contains(&input_ext.as_str());

    if is_image_input {
        let img = image::open(input_path).map_err(|e| format!("Immagine non valida: {}", e))?;
        match target_format {
            "webp" => img
                .save_with_format(&output_path, ImageFormat::WebP)
                .map_err(|e| e.to_string())?,
            "png" => {
                let file = File::create(&output_path).map_err(|e| e.to_string())?;
                let writer = BufWriter::new(file);
                let encoder =
                    PngEncoder::new_with_quality(writer, CompressionType::Best, PngFilter::Adaptive);
                encoder
                    .write_image(
                        img.as_bytes(),
                        img.width(),
                        img.height(),
                        img.color().into(),
                    )
                    .map_err(|e| e.to_string())?;
            }
            "jpg" => {
                let file = File::create(&output_path).map_err(|e| e.to_string())?;
                let mut writer = BufWriter::new(file);
                let mut encoder = JpegEncoder::new_with_quality(&mut writer, 85);
                encoder
                    .encode_image(&img.to_rgb8())
                    .map_err(|e| e.to_string())?;
            }
            "ico" => {
                let icon = img.resize(256, 256, FilterType::Lanczos3);
                icon.save_with_format(&output_path, ImageFormat::Ico)
                    .map_err(|e| e.to_string())?;
            }
            "compress" => {
                let (w, h) = (img.width(), img.height());
                let processed = if w > 1920 || h > 1920 {
                    img.resize(1920, 1920, FilterType::Lanczos3)
                } else {
                    img
                };
                let file = File::create(&output_path).map_err(|e| e.to_string())?;
                let mut writer = BufWriter::new(file);
                let mut encoder = JpegEncoder::new_with_quality(&mut writer, 72);
                encoder
                    .encode_image(&processed.to_rgb8())
                    .map_err(|e| e.to_string())?;
            }
            "pdf" => {
                save_image_as_pdf(&img, &output_path)?;
            }
            _ => return Err("Conversione immagine non supportata".into()),
        }
    } else {
        let raw_text = match input_ext.as_str() {
            "docx" => extract_text_from_docx(input_path)?,
            "pdf" => extract_text_from_pdf(input_path)?,
            _ => fs::read_to_string(input_path)
                .map_err(|_| "File non leggibile come testo".to_string())?,
        };

        if input_ext == "csv" && ["json", "md", "html"].contains(&target_format) {
            let converted = convert_csv_rfc4180(&raw_text, target_format)?;
            fs::write(&output_path, converted).map_err(|e| e.to_string())?;
        } else if input_ext == "json" && target_format == "csv" {
            let csv_out = convert_json_to_csv_rfc4180(&raw_text)?;
            fs::write(&output_path, csv_out).map_err(|e| e.to_string())?;
        } else {
            match target_format {
                "pdf" => save_text_as_pdf(&raw_text, &output_path)?,
                "docx" => create_docx_from_text(&raw_text, &output_path)?,
                "txt" | "md" => fs::write(&output_path, &raw_text).map_err(|e| e.to_string())?,
                "html" => {
                    let paragraphs: String = raw_text
                        .lines()
                        .map(|l| {
                            format!(
                                "<p>{}</p>",
                                l.replace('&', "&amp;").replace('<', "&lt;")
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    let html = format!(
                        "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>{}</title><style>body{{font-family:system-ui,sans-serif;max-width:740px;margin:40px auto;line-height:1.6;padding:0 20px;color:#18181b}}</style></head><body>{}</body></html>",
                        stem, paragraphs
                    );
                    fs::write(&output_path, html).map_err(|e| e.to_string())?;
                }
                _ => return Err("Formato di destinazione non supportato".into()),
            }
        }
    }

    let new_size = fs::metadata(&output_path).map_err(|e| e.to_string())?.len();
    Ok((output_path, old_size, new_size))
}

fn execute_batch_atomic(
    file_paths: &[String],
    target_format: &str,
    output_mode: &str,
) -> Result<MorphResult, String> {
    if file_paths.is_empty() {
        return Err("Nessun file selezionato".into());
    }

    let is_batch = file_paths.len() > 1;

    // 1. Validazione preventiva sull'intero batch prima di scrivere su disco
    for raw_path in file_paths {
        let p = Path::new(raw_path);
        if !p.exists() {
            return Err(format!("File non esistente: {}", raw_path));
        }
        let ext = p
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        if !is_conversion_supported(&ext, target_format, is_batch) {
            return Err(format!(
                "Il file .{} non può essere convertito in .{}",
                ext.to_uppercase(),
                target_format.to_uppercase()
            ));
        }
    }

    // 2. Caso Batch ➔ Unico Archivio ZIP
    if is_batch && target_format == "zip" {
        let paths: Vec<PathBuf> = file_paths.iter().map(PathBuf::from).collect();
        let first = &paths[0];
        let target_dir = resolve_target_dir(first, output_mode);
        let zip_path = get_unique_output_path(first, &target_dir, "Morph_Bundle", "zip", false);

        let mut total_old = 0u64;
        for p in &paths {
            if let Ok(m) = fs::metadata(p) {
                total_old += m.len();
            }
        }
        create_zip_from_files(&paths, &zip_path)?;
        let new_size = fs::metadata(&zip_path).map_err(|e| e.to_string())?.len();
        let out_str = zip_path.to_string_lossy().into_owned();
        return Ok(MorphResult {
            output_path: out_str.clone(),
            output_paths: vec![out_str],
            old_size: total_old,
            new_size,
            files_count: paths.len(),
        });
    }

    // 3. Esecuzione con Rollback Atomico in caso di errore su uno qualsiasi dei file
    let mut total_old_size = 0u64;
    let mut total_new_size = 0u64;
    let mut created_paths: Vec<PathBuf> = Vec::new();

    for raw_path in file_paths {
        let p = Path::new(raw_path);
        match convert_single_file(p, target_format, output_mode) {
            Ok((out_path, old_s, new_s)) => {
                total_old_size += old_s;
                total_new_size += new_s;
                created_paths.push(out_path);
            }
            Err(err) => {
                // Rollback: rimuove tutti i file parziali creati finora nel batch
                for created in &created_paths {
                    if created.is_dir() {
                        let _ = fs::remove_dir_all(created);
                    } else {
                        let _ = fs::remove_file(created);
                    }
                }
                return Err(format!("Batch annullato ({}): nessun file parziale lasciato", err));
            }
        }
    }

    let output_strings: Vec<String> = created_paths
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    let last_str = output_strings.last().cloned().unwrap_or_default();

    Ok(MorphResult {
        output_path: last_str,
        output_paths: output_strings,
        old_size: total_old_size,
        new_size: total_new_size,
        files_count: created_paths.len(),
    })
}

#[tauri::command]
async fn morph_files(
    file_paths: Vec<String>,
    target_format: String,
    output_mode: String,
) -> Result<MorphResult, String> {
    execute_batch_atomic(&file_paths, &target_format, &output_mode)
}

#[tauri::command]
fn reveal_in_folder(paths: Vec<String>) -> Result<(), String> {
    let Some(first) = paths.first() else {
        return Ok(());
    };
    #[cfg(target_os = "windows")]
    {
        if paths.len() == 1 {
            Command::new("explorer")
                .args(["/select,", first])
                .spawn()
                .map_err(|e| e.to_string())?;
        } else {
            let parent = Path::new(first)
                .parent()
                .unwrap_or(Path::new("."))
                .to_string_lossy()
                .into_owned();
            Command::new("explorer")
                .arg(parent)
                .spawn()
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

#[tauri::command]
fn copy_file_to_clipboard(paths: Vec<String>) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;

        let formatted_list: Vec<String> = paths
            .iter()
            .map(|p| format!("'{}'", p.replace('\'', "''")))
            .collect();
        let ps_cmd = format!("Set-Clipboard -LiteralPath @({})", formatted_list.join(","));

        Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &ps_cmd])
            .creation_flags(CREATE_NO_WINDOW)
            .status()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn sync_window_bounds(window: tauri::WebviewWindow, width: f64, height: f64) -> Result<(), String> {
    let scale = window.scale_factor().map_err(|e| e.to_string())?;
    let pos = window.outer_position().map_err(|e| e.to_string())?;
    let size = window.outer_size().map_err(|e| e.to_string())?;

    let target_w = (width * scale).round() as i32;
    let target_h = (height * scale).round() as i32;

    let center_x = pos.x + (size.width as i32) / 2;
    let center_y = pos.y + (size.height as i32) / 2;

    let new_x = center_x - target_w / 2;
    let new_y = center_y - target_h / 2;

    window
        .set_size(tauri::PhysicalSize::new(target_w as u32, target_h as u32))
        .map_err(|e| e.to_string())?;
    window
        .set_position(tauri::PhysicalPosition::new(new_x, new_y))
        .map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
fn update_settings(preset: u8, shortcut_enabled: bool, auto_grab: bool, autostart: bool) {
    SHORTCUT_PRESET.store(preset, Ordering::Relaxed);
    SHORTCUT_ENABLED.store(shortcut_enabled, Ordering::Relaxed);
    AUTO_GRAB_FILE.store(auto_grab, Ordering::Relaxed);

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        use std::process::Stdio;
        const CREATE_NO_WINDOW: u32 = 0x08000000;

        if let Ok(exe_path) = std::env::current_exe() {
            let exe_str = exe_path.to_string_lossy().into_owned();

            if autostart {
                let _ = Command::new("reg")
                    .args([
                        "add",
                        r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                        "/v",
                        "MorphConverter",
                        "/t",
                        "REG_SZ",
                        "/d",
                        &format!("\"{}\"", exe_str),
                        "/f",
                    ])
                    .creation_flags(CREATE_NO_WINDOW)
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .output();
            } else {
                let exists = Command::new("reg")
                    .args([
                        "query",
                        r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                        "/v",
                        "MorphConverter",
                    ])
                    .creation_flags(CREATE_NO_WINDOW)
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .output()
                    .map(|o| o.status.success())
                    .unwrap_or(false);

                if exists {
                    let _ = Command::new("reg")
                        .args([
                            "delete",
                            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                            "/v",
                            "MorphConverter",
                            "/f",
                        ])
                        .creation_flags(CREATE_NO_WINDOW)
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .output();
                }
            }
        }
    }
}

#[tauri::command]
fn hide_window(window: tauri::WebviewWindow) {
    let _ = window.hide();
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let show_item = MenuItem::with_id(app, "show", "Mostra Morph", true, None::<&str>)?;
            let settings_item =
                MenuItem::with_id(app, "settings", "Impostazioni", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "Esci da Morph", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_item, &settings_item, &quit_item])?;

            let mut tray_builder = TrayIconBuilder::new()
                .menu(&menu)
                .tooltip("Morph — Convertitore Rapido (Alt + M)")
                .on_menu_event(|app_handle, event| match event.id.as_ref() {
                    "show" => {
                        if let Some(win) = app_handle.get_webview_window("main") {
                            let _ = win.show();
                            let _ = win.set_focus();
                        }
                    }
                    "settings" => {
                        if let Some(win) = app_handle.get_webview_window("main") {
                            let _ = win.show();
                            let _ = win.set_focus();
                            let _ = app_handle.emit("open-settings", ());
                        }
                    }
                    "quit" => {
                        app_handle.exit(0);
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app_handle = tray.app_handle();
                        if let Some(win) = app_handle.get_webview_window("main") {
                            if win.is_visible().unwrap_or(false) {
                                let _ = win.hide();
                            } else {
                                let _ = win.show();
                                let _ = win.set_focus();
                            }
                        }
                    }
                });

            if let Some(icon) = app.default_window_icon() {
                tray_builder = tray_builder.icon(icon.clone());
            }
            let _tray = tray_builder.build(app)?;

            let app_handle = app.handle().clone();
            #[cfg(target_os = "windows")]
            std::thread::spawn(move || {
                let mut was_pressed = false;
                loop {
                    std::thread::sleep(Duration::from_millis(30));
                    if !SHORTCUT_ENABLED.load(Ordering::Relaxed) {
                        continue;
                    }

                    let preset = SHORTCUT_PRESET.load(Ordering::Relaxed);
                    let (mod_vk, key_vk) = match preset {
                        0 => (0x12, 0x4D),
                        1 => (0x12, 0x20),
                        2 => (0x11, 0x20),
                        _ => (0x12, 0x58),
                    };

                    let is_pressed = unsafe {
                        (GetAsyncKeyState(mod_vk) as u16 & 0x8000) != 0
                            && (GetAsyncKeyState(key_vk) as u16 & 0x8000) != 0
                    };

                    if is_pressed && !was_pressed {
                        if let Some(win) = app_handle.get_webview_window("main") {
                            let is_vis = win.is_visible().unwrap_or(false);
                            let is_foc = win.is_focused().unwrap_or(false);

                            if is_vis && is_foc {
                                let _ = win.hide();
                            } else {
                                let grabbed = if AUTO_GRAB_FILE.load(Ordering::Relaxed) {
                                    get_selected_explorer_files()
                                } else {
                                    Vec::new()
                                };

                                let scale = win.scale_factor().unwrap_or(1.0);
                                let (dim, h_dim) = if !grabbed.is_empty() {
                                    (350.0, 350.0)
                                } else {
                                    (255.0, 70.0)
                                };
                                let _ = win.set_size(tauri::PhysicalSize::new(
                                    (dim * scale) as u32,
                                    (h_dim * scale) as u32,
                                ));
                                let _ = win.center();
                                let _ = win.show();
                                let _ = win.set_focus();
                                let _ = app_handle.emit("shortcut-summon", grabbed);
                            }
                        }
                    }
                    was_pressed = is_pressed;
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            morph_files,
            reveal_in_folder,
            copy_file_to_clipboard,
            sync_window_bounds,
            update_settings,
            hide_window
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

// ============================================================================
// SUITE DI 6 TEST AUTOMATICI (Eseguibile con `cargo test`)
// ============================================================================
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_csv_with_quoted_commas_and_semicolons() {
        let csv_data = "nome,prezzo,note\n\"Mario, Rossi\",42.5,\"Contiene, virgola\"\nLuigi,10,OK";
        let json_out = convert_csv_rfc4180(csv_data, "json").expect("CSV parse failed");
        assert!(json_out.contains("Mario, Rossi"));
        assert!(json_out.contains("Contiene, virgola"));
        assert!(json_out.contains("42.5"));

        let italian_csv = "prodotto;quantita\nCaffè;3";
        let it_json = convert_csv_rfc4180(italian_csv, "json").expect("Italian CSV failed");
        assert!(it_json.contains("Caffè"));
    }

    #[test]
    fn test_nested_json_to_csv_flattening() {
        let nested_json = r#"[
            {"id": 1, "utente": {"nome": "Alice", "ruolo": "Admin"}, "attivo": true},
            {"id": 2, "utente": {"nome": "Bob", "ruolo": "Dev"}, "attivo": false}
        ]"#;
        let csv_out = convert_json_to_csv_rfc4180(nested_json).expect("JSON to CSV failed");
        assert!(csv_out.contains("utente.nome"));
        assert!(csv_out.contains("Alice"));
        assert!(csv_out.contains("Bob"));
    }

    #[test]
    fn test_docx_roundtrip() {
        let temp_dir = std::env::temp_dir();
        let docx_path = temp_dir.join("morph_test_doc.docx");
        let sample_text = "# Titolo Principale\nParagrafo con caratteri speciali: & < > à è";

        create_docx_from_text(sample_text, &docx_path).expect("DOCX creation failed");
        let extracted = extract_text_from_docx(&docx_path).expect("DOCX extraction failed");
        let _ = fs::remove_file(&docx_path);

        assert!(extracted.contains("Titolo Principale"));
        assert!(extracted.contains("Paragrafo con caratteri speciali: & < > à è"));
    }

    #[test]
    fn test_heterogeneous_batch_validation() {
        // PNG + DOCX in WebP deve essere rifiutato preventivamente
        assert!(is_conversion_supported("png", "webp", true));
        assert!(!is_conversion_supported("docx", "webp", true));
        // PNG + DOCX in PDF o ZIP invece è supportato per entrambi!
        assert!(is_conversion_supported("png", "pdf", true));
        assert!(is_conversion_supported("docx", "pdf", true));
    }

    #[test]
    fn test_atomic_batch_rollback_on_corrupt_file() {
        let temp_dir = std::env::temp_dir();
        let valid_txt = temp_dir.join("morph_atomic_valid.txt");
        let corrupt_png = temp_dir.join("morph_atomic_corrupt.png");
        fs::write(&valid_txt, "Testo valido").unwrap();
        fs::write(&corrupt_png, b"NOT_A_VALID_PNG_HEADER").unwrap();

        let batch = vec![
            valid_txt.to_string_lossy().into_owned(),
            corrupt_png.to_string_lossy().into_owned(),
        ];

        // Entrambi supportano "pdf", ma il secondo file è un PNG corrotto:
        // il batch deve fallire e rimuovere il PDF del primo file (rollback atomico)
        let res = execute_batch_atomic(&batch, "pdf", "same");
        assert!(res.is_err());

        let rolled_back_pdf = temp_dir.join("morph_atomic_valid.pdf");
        assert!(!rolled_back_pdf.exists());

        let _ = fs::remove_file(&valid_txt);
        let _ = fs::remove_file(&corrupt_png);
    }

    #[test]
    fn test_multi_file_batch_returns_all_output_paths() {
        let temp_dir = std::env::temp_dir();
        let f1 = temp_dir.join("morph_b1.txt");
        let f2 = temp_dir.join("morph_b2.txt");
        fs::write(&f1, "Primo file").unwrap();
        fs::write(&f2, "Secondo file").unwrap();

        let batch = vec![
            f1.to_string_lossy().into_owned(),
            f2.to_string_lossy().into_owned(),
        ];
        let res = execute_batch_atomic(&batch, "md", "same").expect("Batch MD failed");
        assert_eq!(res.output_paths.len(), 2);
        assert_eq!(res.files_count, 2);

        for out in &res.output_paths {
            let _ = fs::remove_file(out);
        }
        let _ = fs::remove_file(&f1);
        let _ = fs::remove_file(&f2);
    }
}