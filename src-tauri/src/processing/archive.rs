//! Création locale de fichiers CBZ (ZIP32) et PDF à partir d'images.

use image::{codecs::jpeg::JpegEncoder, ImageEncoder};
use std::{
    fs::{self, File, OpenOptions},
    io::{BufReader, BufWriter, Read, Write},
    path::Path,
};
use tauri::AppHandle;

use super::common::{emit_progress, validate_image_size, validate_jpeg_dimensions};

const ZIP32_MAX: u64 = u32::MAX as u64;

fn zip_name(path: &Path, index: usize) -> Vec<u8> {
    let leaf = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("image");
    format!("{:04}_{}", index + 1, leaf)
        .replace('\\', "_")
        .into_bytes()
}

fn write_u16<W: Write>(writer: &mut W, value: u16) -> Result<(), String> {
    writer
        .write_all(&value.to_le_bytes())
        .map_err(|e| e.to_string())
}

fn write_u32<W: Write>(writer: &mut W, value: u32) -> Result<(), String> {
    writer
        .write_all(&value.to_le_bytes())
        .map_err(|e| e.to_string())
}

#[derive(Clone)]
struct ZipEntry {
    name: Vec<u8>,
    crc: u32,
    size: u32,
    offset: u32,
}

#[derive(Default, Clone)]
pub struct ComicMetadata {
    pub title: String,
    pub series: String,
    pub number: String,
    pub writer: String,
    pub summary: String,
    pub year: String,
}

fn xml_escape(value: &str) -> String {
    value
        .chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\r' | '\t'))
        .map(|c| match c {
            '&' => "&amp;".to_owned(),
            '<' => "&lt;".to_owned(),
            '>' => "&gt;".to_owned(),
            '"' => "&quot;".to_owned(),
            '\'' => "&apos;".to_owned(),
            _ => c.to_string(),
        })
        .collect()
}

/// Crée une archive ZIP32 en copiant les images sans les recompresser.
fn export_cbz(
    files: &[String],
    output: &Path,
    app: &AppHandle,
    metadata: Option<&ComicMetadata>,
) -> Result<(), String> {
    if files.is_empty() {
        return Err("Ajoute au moins une image à l'archive.".into());
    }
    if files.len() > u16::MAX as usize {
        return Err("Une archive CBZ ne peut pas contenir plus de 65 535 fichiers.".into());
    }
    let total_bytes: u64 = files
        .iter()
        .map(|path| fs::metadata(path).map(|m| m.len()).unwrap_or(0))
        .sum();
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .map_err(|e| {
            format!(
                "Impossible de créer {} sans remplacer un fichier existant : {e}",
                output.display()
            )
        })?;
    let result = (|| {
        let mut writer = BufWriter::with_capacity(256 * 1024, file);
        let mut entries = Vec::with_capacity(files.len());
        let mut offset = 0u64;
        let mut copied_total = 0u64;
        let mut last_percent = 0;
        for (index, source) in files.iter().enumerate() {
            let path = Path::new(source);
            let name = zip_name(path, index);
            let mut reader = BufReader::new(
                File::open(path)
                    .map_err(|e| format!("Impossible de lire {} : {e}", path.display()))?,
            );
            let metadata = reader.get_ref().metadata().map_err(|e| e.to_string())?;
            if metadata.len() > ZIP32_MAX || name.len() > u16::MAX as usize {
                return Err("Ce fichier dépasse les limites du format CBZ ZIP32.".into());
            }
            if offset > ZIP32_MAX {
                return Err("L'archive dépasse la limite ZIP32 de 4 Go.".into());
            }
            let local_offset = offset as u32;
            write_u32(&mut writer, 0x0403_4b50)?;
            write_u16(&mut writer, 20)?; // Version minimale de ZIP prise en charge.
            write_u16(&mut writer, 0x0808)?; // Active UTF-8 et le descripteur de données.
            write_u16(&mut writer, 0)?; // Stocke les images telles quelles, sans les recompresser.
            write_u16(&mut writer, 0)?;
            write_u16(&mut writer, 0x0021)?; // Date ZIP minimale pour un résultat reproductible.
            write_u32(&mut writer, 0)?;
            write_u32(&mut writer, 0)?;
            write_u32(&mut writer, 0)?;
            write_u16(&mut writer, name.len() as u16)?;
            write_u16(&mut writer, 0)?;
            writer.write_all(&name).map_err(|e| e.to_string())?;
            offset += 30 + name.len() as u64;

            let mut hasher = crc32fast::Hasher::new();
            let mut size = 0u64;
            let mut buffer = [0u8; 128 * 1024];
            loop {
                let count = reader
                    .read(&mut buffer)
                    .map_err(|e| format!("Erreur de lecture de {} : {e}", path.display()))?;
                if count == 0 {
                    break;
                }
                writer
                    .write_all(&buffer[..count])
                    .map_err(|e| e.to_string())?;
                hasher.update(&buffer[..count]);
                size += count as u64;
                offset += count as u64;
                copied_total += count as u64;
                if offset > ZIP32_MAX {
                    return Err("L'archive dépasse la limite ZIP32 de 4 Go.".into());
                }
                let percent = if total_bytes == 0 {
                    0
                } else {
                    (copied_total as f32 / total_bytes as f32 * 100.0) as u8
                };
                if percent > last_percent {
                    emit_progress(
                        app,
                        percent as f32,
                        format!("Création CBZ : {} / {}", index + 1, files.len()),
                    );
                    last_percent = percent;
                }
            }
            let crc = hasher.finalize();
            if size > ZIP32_MAX {
                return Err("Ce fichier dépasse les limites du format CBZ ZIP32.".into());
            }
            write_u32(&mut writer, 0x0807_4b50)?;
            write_u32(&mut writer, crc)?;
            write_u32(&mut writer, size as u32)?;
            write_u32(&mut writer, size as u32)?;
            offset += 16;
            entries.push(ZipEntry {
                name,
                crc,
                size: size as u32,
                offset: local_offset,
            });
        }
        if let Some(metadata) = metadata {
            let fields = [
                ("Title", &metadata.title),
                ("Series", &metadata.series),
                ("Number", &metadata.number),
                ("Writer", &metadata.writer),
                ("Summary", &metadata.summary),
                ("Year", &metadata.year),
            ];
            let mut xml = String::from("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<ComicInfo xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xmlns:xsd=\"http://www.w3.org/2001/XMLSchema\">\n");
            for (tag, value) in fields {
                if !value.trim().is_empty() {
                    xml.push_str(&format!("  <{tag}>{}</{tag}>\n", xml_escape(value.trim())));
                }
            }
            xml.push_str("</ComicInfo>\n");
            let name = b"ComicInfo.xml".to_vec();
            let bytes = xml.as_bytes();
            let size = u32::try_from(bytes.len())
                .map_err(|_| "Les métadonnées sont trop volumineuses.".to_string())?;
            let entry_offset = u32::try_from(offset)
                .map_err(|_| "L'archive dépasse la limite ZIP32 de 4 Go.".to_string())?;
            let crc = crc32fast::hash(bytes);
            write_u32(&mut writer, 0x0403_4b50)?;
            write_u16(&mut writer, 20)?;
            write_u16(&mut writer, 0x0808)?;
            write_u16(&mut writer, 0)?;
            write_u16(&mut writer, 0)?;
            write_u16(&mut writer, 0x0021)?;
            write_u32(&mut writer, 0)?;
            write_u32(&mut writer, 0)?;
            write_u32(&mut writer, 0)?;
            write_u16(&mut writer, name.len() as u16)?;
            write_u16(&mut writer, 0)?;
            writer.write_all(&name).map_err(|e| e.to_string())?;
            writer.write_all(bytes).map_err(|e| e.to_string())?;
            write_u32(&mut writer, 0x0807_4b50)?;
            write_u32(&mut writer, crc)?;
            write_u32(&mut writer, size)?;
            write_u32(&mut writer, size)?;
            offset += 30 + name.len() as u64 + bytes.len() as u64 + 16;
            entries.push(ZipEntry {
                name,
                crc,
                size,
                offset: entry_offset,
            });
        }
        if offset > ZIP32_MAX {
            return Err("L'archive dépasse la limite ZIP32 de 4 Go.".into());
        }
        let central_offset = offset as u32;
        for entry in &entries {
            write_u32(&mut writer, 0x0201_4b50)?;
            write_u16(&mut writer, 20)?;
            write_u16(&mut writer, 20)?;
            write_u16(&mut writer, 0x0808)?;
            write_u16(&mut writer, 0)?;
            write_u16(&mut writer, 0)?;
            write_u16(&mut writer, 0x0021)?;
            write_u32(&mut writer, entry.crc)?;
            write_u32(&mut writer, entry.size)?;
            write_u32(&mut writer, entry.size)?;
            write_u16(&mut writer, entry.name.len() as u16)?;
            for _ in 0..4 {
                write_u16(&mut writer, 0)?;
            }
            write_u32(&mut writer, 0)?;
            write_u32(&mut writer, entry.offset)?;
            writer.write_all(&entry.name).map_err(|e| e.to_string())?;
            offset += 46 + entry.name.len() as u64;
        }
        let central_size = offset - central_offset as u64;
        if central_size > ZIP32_MAX {
            return Err("Index central CBZ trop volumineux.".into());
        }
        write_u32(&mut writer, 0x0605_4b50)?;
        write_u16(&mut writer, 0)?;
        write_u16(&mut writer, 0)?;
        write_u16(&mut writer, entries.len() as u16)?;
        write_u16(&mut writer, entries.len() as u16)?;
        write_u32(&mut writer, central_size as u32)?;
        write_u32(&mut writer, central_offset)?;
        write_u16(&mut writer, 0)?;
        writer.flush().map_err(|e| e.to_string())
    })();
    if let Err(error) = result {
        let _ = fs::remove_file(output);
        return Err(error);
    }
    Ok(())
}

fn write_pdf_object<W: Write>(
    writer: &mut W,
    offset: &mut u64,
    id: u32,
    body: &[u8],
) -> Result<(), String> {
    let head = format!("{id} 0 obj\n");
    writer
        .write_all(head.as_bytes())
        .map_err(|e| e.to_string())?;
    writer.write_all(body).map_err(|e| e.to_string())?;
    writer.write_all(b"\nendobj\n").map_err(|e| e.to_string())?;
    *offset += head.len() as u64 + body.len() as u64 + 8;
    Ok(())
}

/// Construit un PDF image par image en conservant une mémoire bornée.
fn export_pdf(files: &[String], output: &Path, app: &AppHandle) -> Result<(), String> {
    if files.is_empty() {
        return Err("Ajoute au moins une image au PDF.".into());
    }
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .map_err(|e| {
            format!(
                "Impossible de créer {} sans remplacer un fichier existant : {e}",
                output.display()
            )
        })?;
    let result = (|| {
        let mut writer = BufWriter::with_capacity(256 * 1024, file);
        let mut offset = 0u64;
        let mut offsets = vec![0u64; files.len() * 3 + 3];
        let header = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n";
        writer.write_all(header).map_err(|e| e.to_string())?;
        offset += header.len() as u64;
        let page_refs = (0..files.len())
            .map(|index| format!("{} 0 R", index as u32 * 3 + 5))
            .collect::<Vec<_>>()
            .join(" ");
        offsets[1] = offset;
        write_pdf_object(
            &mut writer,
            &mut offset,
            1,
            b"<< /Type /Catalog /Pages 2 0 R >>",
        )?;
        offsets[2] = offset;
        write_pdf_object(
            &mut writer,
            &mut offset,
            2,
            format!(
                "<< /Type /Pages /Count {} /Kids [{page_refs}] >>",
                files.len()
            )
            .as_bytes(),
        )?;

        for (index, source) in files.iter().enumerate() {
            let decoded =
                image::open(source).map_err(|e| format!("Impossible d'ouvrir {} : {e}", source))?;
            validate_image_size(decoded.width(), decoded.height())?;
            let width = decoded.width();
            let height = decoded.height();
            validate_jpeg_dimensions(width, height)?;
            let rgb = decoded.into_rgb8();
            let mut jpeg = Vec::new();
            JpegEncoder::new_with_quality(&mut jpeg, 90)
                .write_image(rgb.as_raw(), width, height, image::ExtendedColorType::Rgb8)
                .map_err(|e| e.to_string())?;
            let image_id = index as u32 * 3 + 3;
            let content_id = image_id + 1;
            let page_id = image_id + 2;
            let scale = (14_400.0f64 / (width.max(height) as f64 * 0.24)).min(1.0);
            // Convertit les pixels en points PDF et réduit les grandes pages
            // pour rester sous la taille maximale d'une page PDF.
            let page_width = width as f64 * 0.24 * scale;
            let page_height = height as f64 * 0.24 * scale;

            offsets[image_id as usize] = offset;
            let image_header = format!("<< /Type /XObject /Subtype /Image /Width {width} /Height {height} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode /Length {} >>\nstream\n", jpeg.len());
            let object_head = format!("{image_id} 0 obj\n");
            writer
                .write_all(object_head.as_bytes())
                .map_err(|e| e.to_string())?;
            writer
                .write_all(image_header.as_bytes())
                .map_err(|e| e.to_string())?;
            writer.write_all(&jpeg).map_err(|e| e.to_string())?;
            writer
                .write_all(b"\nendstream\nendobj\n")
                .map_err(|e| e.to_string())?;
            offset += object_head.len() as u64 + image_header.len() as u64 + jpeg.len() as u64 + 18;

            let content = format!("q\n{page_width:.2} 0 0 {page_height:.2} 0 0 cm\n/Im0 Do\nQ\n");
            offsets[content_id as usize] = offset;
            write_pdf_object(
                &mut writer,
                &mut offset,
                content_id,
                format!(
                    "<< /Length {} >>\nstream\n{}endstream",
                    content.len(),
                    content
                )
                .as_bytes(),
            )?;
            offsets[page_id as usize] = offset;
            let page = format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {page_width:.2} {page_height:.2}] /Resources << /XObject << /Im0 {image_id} 0 R >> >> /Contents {content_id} 0 R >>");
            write_pdf_object(&mut writer, &mut offset, page_id, page.as_bytes())?;
            emit_progress(
                app,
                (index + 1) as f32 / files.len() as f32 * 95.0,
                format!("Création PDF : {} / {}", index + 1, files.len()),
            );
        }
        let xref = offset;
        writer
            .write_all(format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len()).as_bytes())
            .map_err(|e| e.to_string())?;
        for entry in offsets.iter().skip(1) {
            writer
                .write_all(format!("{entry:010} 00000 n \n").as_bytes())
                .map_err(|e| e.to_string())?;
        }
        writer
            .write_all(
                format!(
                    "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                    offsets.len()
                )
                .as_bytes(),
            )
            .map_err(|e| e.to_string())?;
        writer.flush().map_err(|e| e.to_string())
    })();
    if let Err(error) = result {
        let _ = fs::remove_file(output);
        return Err(error);
    }
    Ok(())
}

/// Choisit le générateur selon l'extension et transmet les métadonnées CBZ éventuelles.
pub fn export(
    files: &[String],
    output: &Path,
    app: &AppHandle,
    metadata: Option<&ComicMetadata>,
) -> Result<(), String> {
    match output
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "cbz" => export_cbz(files, output, app, metadata),
        "pdf" => export_pdf(files, output, app),
        _ => Err("Choisis une extension PDF ou CBZ.".into()),
    }
}
