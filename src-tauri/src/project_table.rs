//! Host-only parsing of raw, bounded logical CSV/TSV records.
//! Public text sanitization and display truncation belong to source projection.

use omicsops_dto::{TablePreviewErrorCode, TablePreviewTruncation};

pub(crate) const TABLE_PREVIEW_MAX_BYTES: usize = 256 * 1024;
const MAX_ROWS: usize = 200;
const MAX_COLUMNS: usize = 64;

#[derive(Debug, Clone, Copy)]
pub(crate) enum TableDelimiter {
    Csv,
    Tsv,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ParsedProjectTable {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
    /// Raw byte offset after the last completed record, including an initial BOM.
    pub parsed_bytes: u64,
    pub truncation_reasons: Vec<TablePreviewTruncation>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FieldState {
    Start,
    Unquoted,
    Quoted,
    ClosedQuote,
}

pub(crate) fn parse_project_table(
    bytes: &[u8],
    delimiter: TableDelimiter,
    eof: bool,
) -> Result<ParsedProjectTable, TablePreviewErrorCode> {
    let capped = bytes.len() > TABLE_PREVIEW_MAX_BYTES;
    let bytes = &bytes[..bytes.len().min(TABLE_PREVIEW_MAX_BYTES)];
    let eof = eof && !capped;
    let separator = match delimiter {
        TableDelimiter::Csv => b',',
        TableDelimiter::Tsv => b'\t',
    };
    let mut offset = if bytes.starts_with(b"\xef\xbb\xbf") {
        3
    } else {
        0
    };
    let mut result = ParsedProjectTable {
        headers: Vec::new(),
        rows: Vec::new(),
        parsed_bytes: offset as u64,
        truncation_reasons: Vec::new(),
    };
    if capped || (!eof && bytes.len() == TABLE_PREVIEW_MAX_BYTES) {
        result
            .truncation_reasons
            .push(TablePreviewTruncation::ByteLimit);
    }
    let mut header_seen = false;
    let mut record_start = offset;
    let mut state = FieldState::Start;
    let mut field = Vec::new();
    let mut fields = Vec::new();
    while offset < bytes.len() {
        let byte = bytes[offset];
        if state == FieldState::Quoted {
            if byte == b'"' {
                state = FieldState::ClosedQuote;
            } else {
                field.push(byte);
            }
            offset += 1;
            continue;
        }
        if state == FieldState::ClosedQuote && byte == b'"' {
            field.push(b'"');
            state = FieldState::Quoted;
            offset += 1;
            continue;
        }
        if byte == separator {
            push_field(&mut fields, &mut field)?;
            // A separator after the 64th field starts an observed 65th field.
            if fields.len() == MAX_COLUMNS {
                return Err(TablePreviewErrorCode::ColumnLimit);
            }
            state = FieldState::Start;
            offset += 1;
            continue;
        }
        if byte == b'\n' || byte == b'\r' {
            let end = if byte == b'\r' {
                if offset + 1 == bytes.len() && !eof {
                    break;
                }
                if bytes.get(offset + 1) != Some(&b'\n') {
                    return Err(TablePreviewErrorCode::MalformedTable);
                }
                offset + 2
            } else {
                offset + 1
            };
            push_field(&mut fields, &mut field)?;
            complete_record(&mut result, &mut header_seen, &mut fields, end)?;
            offset = end;
            record_start = end;
            state = FieldState::Start;
            // Do not inspect or validate any bytes beyond the row budget.
            if result.rows.len() == MAX_ROWS && (offset < bytes.len() || !eof) {
                result
                    .truncation_reasons
                    .push(TablePreviewTruncation::RowLimit);
                return Ok(result);
            }
            continue;
        }
        match state {
            FieldState::Start if byte == b'"' => state = FieldState::Quoted,
            FieldState::ClosedQuote => return Err(TablePreviewErrorCode::MalformedTable),
            _ if byte == b'"' => return Err(TablePreviewErrorCode::MalformedTable),
            _ => {
                state = FieldState::Unquoted;
                field.push(byte);
            }
        }
        offset += 1;
    }
    if eof {
        // Validate raw observed bytes even when an unterminated quote follows.
        std::str::from_utf8(&bytes[record_start..])
            .map_err(|_| TablePreviewErrorCode::InvalidUtf8)?;
        if state == FieldState::Quoted {
            return Err(TablePreviewErrorCode::MalformedTable);
        }
        if record_start < bytes.len() {
            push_field(&mut fields, &mut field)?;
            complete_record(&mut result, &mut header_seen, &mut fields, bytes.len())?;
        }
    } else if record_start < bytes.len() || !header_seen {
        result
            .truncation_reasons
            .push(TablePreviewTruncation::IncompleteRecord);
        if let Err(error) = std::str::from_utf8(&bytes[record_start..]) {
            if error.error_len().is_some() {
                return Err(TablePreviewErrorCode::InvalidUtf8);
            }
            result
                .truncation_reasons
                .push(TablePreviewTruncation::IncompleteUtf8);
        }
    }
    Ok(result)
}

fn push_field(fields: &mut Vec<String>, field: &mut Vec<u8>) -> Result<(), TablePreviewErrorCode> {
    let text =
        String::from_utf8(std::mem::take(field)).map_err(|_| TablePreviewErrorCode::InvalidUtf8)?;
    fields.push(text);
    Ok(())
}

fn complete_record(
    result: &mut ParsedProjectTable,
    header_seen: &mut bool,
    fields: &mut Vec<String>,
    end: usize,
) -> Result<(), TablePreviewErrorCode> {
    if *header_seen {
        if fields.len() != result.headers.len() {
            return Err(TablePreviewErrorCode::MalformedTable);
        }
        result.rows.push(std::mem::take(fields));
    } else {
        result.headers = std::mem::take(fields);
        *header_seen = true;
    }
    result.parsed_bytes = end as u64;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use omicsops_dto::{TablePreviewErrorCode, TablePreviewTruncation};

    #[test]
    fn table_preview_record_boundaries() {
        let parsed = parse_project_table(
            b"id,note\r\n1,\"a\nb\"\r\n2,unfinished",
            TableDelimiter::Csv,
            false,
        )
        .unwrap();
        assert_eq!(parsed.headers, vec!["id", "note"]);
        assert_eq!(parsed.rows, vec![vec!["1", "a\nb"]]);
        assert_eq!(
            parsed.truncation_reasons,
            vec![TablePreviewTruncation::IncompleteRecord]
        );
        assert_eq!(parsed.parsed_bytes, 18);
        assert_eq!(
            parse_project_table(b"id\n\"unfinished", TableDelimiter::Csv, true).unwrap_err(),
            TablePreviewErrorCode::MalformedTable
        );
    }

    #[test]
    fn bom_chinese_crlf_escaped_quotes_and_quoted_delimiters() {
        let bytes = "\u{feff}id,note\r\n1,\"中文,\"\"引用\"\"\"\r\n".as_bytes();
        let parsed = parse_project_table(bytes, TableDelimiter::Csv, true).unwrap();
        assert_eq!(parsed.headers, vec!["id", "note"]);
        assert_eq!(parsed.rows, vec![vec!["1", "中文,\"引用\""]]);
        assert_eq!(parsed.parsed_bytes, 35);
        assert!(parsed.truncation_reasons.is_empty());
    }

    #[test]
    fn tsv_uses_tabs_and_preserves_comma_and_quoted_tab() {
        let parsed =
            parse_project_table(b"id\tnote\n1\t\"a,b\tc\"", TableDelimiter::Tsv, true).unwrap();
        assert_eq!(parsed.rows, vec![vec!["1", "a,b\tc"]]);
        assert_eq!(parsed.parsed_bytes, 17);
    }

    #[test]
    fn empty_and_header_only_are_valid_without_fabricated_records() {
        for bytes in [b"".as_slice(), b"\xef\xbb\xbf"] {
            let parsed = parse_project_table(bytes, TableDelimiter::Csv, true).unwrap();
            assert!(parsed.headers.is_empty());
            assert!(parsed.rows.is_empty());
            assert_eq!(parsed.parsed_bytes, bytes.len() as u64);
            assert!(parsed.truncation_reasons.is_empty());
        }
        for bytes in [b"id,note".as_slice(), b"id,note\n"] {
            let parsed = parse_project_table(bytes, TableDelimiter::Csv, true).unwrap();
            assert_eq!(parsed.headers, vec!["id", "note"]);
            assert!(parsed.rows.is_empty());
            assert_eq!(parsed.parsed_bytes, bytes.len() as u64);
            assert!(parsed.truncation_reasons.is_empty());
        }
    }

    #[test]
    fn eof_final_record_and_empty_fields_are_complete() {
        let parsed = parse_project_table(b"a,b,c\n1,,", TableDelimiter::Csv, true).unwrap();
        assert_eq!(parsed.rows, vec![vec!["1", "", ""]]);
        assert_eq!(parsed.parsed_bytes, 9);
    }

    #[test]
    fn malformed_observed_grammar_and_record_width_are_rejected() {
        for bytes in [
            b"a,b\n1\n".as_slice(),
            b"a\n1,2\n",
            b"a\nx\"y\n",
            b"a\n\"x\"y\n",
            b"a\nx\ry\n",
            b"a\nx\r",
        ] {
            assert_eq!(
                parse_project_table(bytes, TableDelimiter::Csv, true).unwrap_err(),
                TablePreviewErrorCode::MalformedTable
            );
        }
        assert_eq!(
            parse_project_table(b"a\n\"x\"bad", TableDelimiter::Csv, false).unwrap_err(),
            TablePreviewErrorCode::MalformedTable
        );
        assert_eq!(
            parse_project_table(b"a,b\n1\n2,", TableDelimiter::Csv, false).unwrap_err(),
            TablePreviewErrorCode::MalformedTable
        );
    }

    #[test]
    fn incomplete_prefix_discards_record_and_retains_raw_complete_offset() {
        let parsed =
            parse_project_table(b"\xef\xbb\xbfa\n1\n\"tail", TableDelimiter::Csv, false).unwrap();
        assert_eq!(parsed.rows, vec![vec!["1"]]);
        assert_eq!(parsed.parsed_bytes, 7);
        assert_eq!(
            parsed.truncation_reasons,
            vec![TablePreviewTruncation::IncompleteRecord]
        );
        let split_crlf = parse_project_table(b"a\r\n1\r", TableDelimiter::Csv, false).unwrap();
        assert_eq!(split_crlf.parsed_bytes, 3);
        assert!(split_crlf.rows.is_empty());
    }

    #[test]
    fn only_a_prefix_trailing_partial_utf8_sequence_is_allowed() {
        let parsed = parse_project_table(b"a\n1\n\xe4\xb8", TableDelimiter::Csv, false).unwrap();
        assert_eq!(parsed.rows, vec![vec!["1"]]);
        assert_eq!(parsed.parsed_bytes, 4);
        assert_eq!(
            parsed.truncation_reasons,
            vec![
                TablePreviewTruncation::IncompleteRecord,
                TablePreviewTruncation::IncompleteUtf8
            ]
        );
        for (bytes, eof) in [
            (b"a\n\xe4\xb8".as_slice(), true),
            (b"a\n\xfftail", false),
            (b"a\n\xe4\xb8\n", false),
            (b"a\n\xff\n", true),
        ] {
            assert_eq!(
                parse_project_table(bytes, TableDelimiter::Csv, eof).unwrap_err(),
                TablePreviewErrorCode::InvalidUtf8
            );
        }
    }

    #[test]
    fn column_budget_accepts_64_and_rejects_observed_65th_column() {
        let header = vec!["h"; 64].join(",");
        let parsed = parse_project_table(header.as_bytes(), TableDelimiter::Csv, true).unwrap();
        assert_eq!(parsed.headers.len(), 64);
        let too_wide = vec!["h"; 65].join(",");
        for eof in [true, false] {
            assert_eq!(
                parse_project_table(too_wide.as_bytes(), TableDelimiter::Csv, eof).unwrap_err(),
                TablePreviewErrorCode::ColumnLimit
            );
        }
        let wide_row = format!("{header}\n{}", vec!["v"; 65].join(","));
        assert_eq!(
            parse_project_table(wide_row.as_bytes(), TableDelimiter::Csv, true).unwrap_err(),
            TablePreviewErrorCode::ColumnLimit
        );
    }

    #[test]
    fn row_budget_stops_before_unobserved_invalid_tail() {
        let exactly_200 = format!("id\n{}", "1\n".repeat(200));
        let parsed =
            parse_project_table(exactly_200.as_bytes(), TableDelimiter::Csv, true).unwrap();
        assert_eq!(parsed.rows.len(), 200);
        assert_eq!(parsed.parsed_bytes, 403);
        assert!(parsed.truncation_reasons.is_empty());
        for tail in [b"2\n".as_slice(), b"\xff\n", b"\"bad", b"1,2\n"] {
            let bytes = [exactly_200.as_bytes(), tail].concat();
            let parsed = parse_project_table(&bytes, TableDelimiter::Csv, true).unwrap();
            assert_eq!(parsed.rows.len(), 200);
            assert_eq!(parsed.parsed_bytes, 403);
            assert_eq!(
                parsed.truncation_reasons,
                vec![TablePreviewTruncation::RowLimit]
            );
        }
        let parsed =
            parse_project_table(exactly_200.as_bytes(), TableDelimiter::Csv, false).unwrap();
        assert_eq!(
            parsed.truncation_reasons,
            vec![TablePreviewTruncation::RowLimit]
        );
    }

    #[test]
    fn raw_fields_over_display_budget_remain_intact_for_later_projection() {
        let field = "中".repeat(1400);
        let bytes = format!("{field}\n\"{field}\"\n");
        let parsed = parse_project_table(bytes.as_bytes(), TableDelimiter::Csv, true).unwrap();
        assert_eq!(parsed.headers, vec![field.clone()]);
        assert_eq!(parsed.rows, vec![vec![field]]);
        assert!(parsed.truncation_reasons.is_empty());
    }

    #[test]
    fn byte_budget_clamps_input_and_treats_cut_utf8_as_prefix() {
        let bytes = format!("h\n{}中\n", "x".repeat(262141));
        let parsed = parse_project_table(bytes.as_bytes(), TableDelimiter::Csv, true).unwrap();
        assert_eq!(parsed.headers, vec!["h"]);
        assert!(parsed.rows.is_empty());
        assert_eq!(parsed.parsed_bytes, 2);
        assert_eq!(
            parsed.truncation_reasons,
            vec![
                TablePreviewTruncation::ByteLimit,
                TablePreviewTruncation::IncompleteRecord,
                TablePreviewTruncation::IncompleteUtf8
            ]
        );
    }
}
