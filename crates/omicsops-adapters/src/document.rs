use std::{fs::File, io::Read, path::Path};

use quick_xml::{Reader, events::Event};
use serde::{Deserialize, Serialize};
use zip::ZipArchive;

use crate::{AdapterError, AdapterResult};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractedPlan {
    pub format: String,
    pub text: String,
}

pub fn extract_plan_text(path: &Path) -> AdapterResult<ExtractedPlan> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    let (format, text) = match extension.as_str() {
        "md" | "markdown" | "txt" => ("markdown", std::fs::read_to_string(path)?),
        "docx" => ("docx", extract_docx(path)?),
        "pdf" => (
            "pdf",
            pdf_extract::extract_text(path)
                .map_err(|error| AdapterError::Document(error.to_string()))?,
        ),
        _ => {
            return Err(AdapterError::UnsupportedDocument(
                path.display().to_string(),
            ));
        }
    };

    let text = normalize_text(&text);
    if text.is_empty() {
        return Err(AdapterError::Document(
            "the document contains no extractable text; OCR is not supported".into(),
        ));
    }
    Ok(ExtractedPlan {
        format: format.into(),
        text,
    })
}

fn extract_docx(path: &Path) -> AdapterResult<String> {
    let file = File::open(path)?;
    let mut archive =
        ZipArchive::new(file).map_err(|error| AdapterError::Document(error.to_string()))?;
    let mut xml = String::new();
    archive
        .by_name("word/document.xml")
        .map_err(|error| AdapterError::Document(error.to_string()))?
        .read_to_string(&mut xml)?;

    let mut reader = Reader::from_str(&xml);
    let mut output = String::new();
    loop {
        match reader.read_event() {
            Ok(Event::Text(text)) => {
                let decoded = text
                    .decode()
                    .map_err(|error| AdapterError::Document(error.to_string()))?;
                output.push_str(&decoded);
            }
            Ok(Event::End(tag)) if tag.name().as_ref() == b"w:p" => output.push('\n'),
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => return Err(AdapterError::Document(error.to_string())),
        }
    }
    Ok(output)
}

fn normalize_text(text: &str) -> String {
    let mut lines = Vec::new();
    let mut previous_blank = false;
    for raw in text.replace("\r\n", "\n").replace('\r', "\n").lines() {
        let line = raw.trim();
        let blank = line.is_empty();
        if blank && (previous_blank || lines.is_empty()) {
            continue;
        }
        lines.push(line.to_owned());
        previous_blank = blank;
    }
    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    lines.join("\n")
}
