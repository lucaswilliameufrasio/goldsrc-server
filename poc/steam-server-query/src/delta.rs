//! Bounded local delta schema preparation, not yet used for live sign-on.
use std::fs;
use std::io::{self, Error, ErrorKind, Read};
use std::path::Path;

const LIMIT: usize = 65_536;

#[derive(Debug, Clone)]
struct Field {
    name: String,
    kind: u32,
    offset: u16,
    size: u8,
    bits: u8,
    pre: f32,
    post: f32,
}

#[derive(Debug, Clone)]
struct Schema {
    name: String,
    fields: Vec<Field>,
}

fn invalid() -> Error {
    Error::new(ErrorKind::InvalidData, "invalid delta description input")
}

fn name_ok(name: &str) -> bool {
    !name.is_empty() && name.len() <= 31 && name.bytes().all(|b| b.is_ascii_graphic())
}

fn parse(input: &str) -> io::Result<Vec<Schema>> {
    if input.len() > LIMIT {
        return Err(invalid());
    }
    let mut lines = input.lines();
    let mut schemas = Vec::new();
    while let Some(line) = lines.next() {
        let parts: Vec<_> = line.split('\t').collect();
        if parts.len() != 3 || parts[0] != "schema" || !name_ok(parts[1]) || schemas.len() >= 32 {
            return Err(invalid());
        }
        let count: usize = parts[2].parse().map_err(|_| invalid())?;
        if count == 0 || count > 64 {
            return Err(invalid());
        }
        let mut fields = Vec::with_capacity(count);
        for _ in 0..count {
            let parts: Vec<_> = lines.next().ok_or_else(invalid)?.split('\t').collect();
            if parts.len() != 8 || parts[0] != "field" || !name_ok(parts[1]) {
                return Err(invalid());
            }
            let field = Field {
                name: parts[1].to_owned(),
                kind: parts[2].parse().map_err(|_| invalid())?,
                offset: parts[3].parse().map_err(|_| invalid())?,
                size: parts[4].parse().map_err(|_| invalid())?,
                bits: parts[5].parse().map_err(|_| invalid())?,
                pre: parts[6].parse().map_err(|_| invalid())?,
                post: parts[7].parse().map_err(|_| invalid())?,
            };
            if !field.pre.is_finite()
                || !field.post.is_finite()
                || field.pre < 0.0
                || field.post < 0.0
                || f64::from(field.pre) * 4000.0 > f64::from(u32::MAX)
                || f64::from(field.post) * 4000.0 > f64::from(u32::MAX)
            {
                return Err(invalid());
            }
            fields.push(field);
        }
        schemas.push(Schema {
            name: parts[1].to_owned(),
            fields,
        });
    }
    if schemas.is_empty() {
        return Err(invalid());
    }
    Ok(schemas)
}

struct Writer {
    data: Vec<u8>,
    bit: usize,
}

impl Writer {
    fn bits(&mut self, value: u32, count: usize) -> io::Result<()> {
        for i in 0..count {
            if self.bit / 8 >= LIMIT {
                return Err(invalid());
            }
            if self.bit.is_multiple_of(8) {
                self.data.push(0);
            }
            self.data[self.bit / 8] |= (((value >> i) & 1) as u8) << (self.bit % 8);
            self.bit += 1;
        }
        Ok(())
    }

    fn string(&mut self, value: &str) -> io::Result<()> {
        for b in value.bytes().chain(std::iter::once(0)) {
            self.bits(u32::from(b), 8)?;
        }
        Ok(())
    }

    fn align(&mut self) {
        self.bit = self.bit.next_multiple_of(8);
    }
}

fn encode(schemas: &[Schema]) -> io::Result<Vec<u8>> {
    let mut w = Writer {
        data: Vec::new(),
        bit: 0,
    };
    for schema in schemas {
        w.bits(14, 8)?;
        w.string(&schema.name)?;
        w.bits(schema.fields.len() as u32, 16)?;
        for field in &schema.fields {
            let values = [
                field.kind != 0,
                !field.name.is_empty(),
                field.offset != 0,
                field.size != 0,
                field.bits != 0,
                field.pre != 0.0,
                field.post != 0.0,
            ];
            let mut mask = 0_u32;
            for (index, present) in values.iter().enumerate() {
                if *present {
                    mask |= 1 << index;
                }
            }
            w.bits(u32::from(mask != 0), 3)?;
            if mask != 0 {
                w.bits(mask, 8)?;
            }
            if values[0] {
                w.bits(field.kind, 32)?;
            }
            if values[1] {
                w.string(&field.name)?;
            }
            if values[2] {
                w.bits(u32::from(field.offset), 16)?;
            }
            if values[3] {
                w.bits(u32::from(field.size), 8)?;
            }
            if values[4] {
                w.bits(u32::from(field.bits), 8)?;
            }
            if values[5] {
                w.bits((f64::from(field.pre) * 4000.0) as u32, 32)?;
            }
            if values[6] {
                w.bits((f64::from(field.post) * 4000.0) as u32, 32)?;
            }
        }
        w.align();
    }
    Ok(w.data)
}

/// Prepared descriptions only: callbacks are not executed and no wire data is sent.
pub struct PreparedDelta {
    pub bytes: Vec<u8>,
    pub schemas: usize,
    pub fields: usize,
    pub callbacks: Vec<String>,
}

fn read_bounded(path: &Path) -> io::Result<String> {
    let file = fs::File::open(path)?;
    if !file.metadata()?.is_file() {
        return Err(invalid());
    }
    let mut bytes = Vec::new();
    file.take((LIMIT + 1) as u64).read_to_end(&mut bytes)?;
    if bytes.len() > LIMIT {
        return Err(invalid());
    }
    String::from_utf8(bytes).map_err(|_| invalid())
}

/// Load an explicit local delta.lst and matching runtime offset catalog.
pub fn load(list: &Path, catalog: &Path) -> io::Result<PreparedDelta> {
    let list = read_bounded(list)?;
    let catalog = read_bounded(catalog)?;
    prepare(&list, &catalog)
}

fn prepare(list: &str, catalog: &str) -> io::Result<PreparedDelta> {
    if list.len() > LIMIT {
        return Err(invalid());
    }
    let template = parse(catalog)?;
    let mut found: Vec<Schema> = Vec::new();
    let mut callbacks = Vec::new();
    let mut current: Option<Schema> = None;
    let mut in_body = false;
    for line in list.lines() {
        let line = line.split("//").next().ok_or_else(invalid)?.trim();
        if line.is_empty() {
            continue;
        }
        if current.is_none() {
            let header: Vec<_> = line.split_whitespace().collect();
            if header.len() < 2 || header.len() > 3 || !name_ok(header[0]) {
                return Err(invalid());
            }
            match header.as_slice() {
                [_, "none"] => {}
                [_, "gamedll" | "clientdll", name] if name_ok(name) => {
                    callbacks.push((*name).to_owned())
                }
                _ => return Err(invalid()),
            }
            if found.iter().any(|s| s.name.eq_ignore_ascii_case(header[0])) {
                return Err(invalid());
            }
            current = Some(Schema {
                name: header[0].to_owned(),
                fields: Vec::new(),
            });
        } else if !in_body {
            if line != "{" {
                return Err(invalid());
            }
            in_body = true;
        } else if line == "}" {
            found.push(current.take().ok_or_else(invalid)?);
            in_body = false;
        } else {
            let schema = current.as_mut().ok_or_else(invalid)?;
            let source = template
                .iter()
                .find(|s| s.name.eq_ignore_ascii_case(&schema.name))
                .ok_or_else(invalid)?;
            let (macro_name, rest) = line.split_once('(').ok_or_else(invalid)?;
            let (args, tail) = rest.split_once(')').ok_or_else(invalid)?;
            if !matches!(tail.trim(), "" | ",") {
                return Err(invalid());
            }
            let args: Vec<_> = args.split(',').map(str::trim).collect();
            let post: f32 = match macro_name.trim() {
                "DEFINE_DELTA" if args.len() == 4 => 1.0,
                "DEFINE_DELTA_POST" if args.len() == 5 => args[4].parse().map_err(|_| invalid())?,
                _ => return Err(invalid()),
            };
            let reference = source
                .fields
                .iter()
                .find(|f| f.name.eq_ignore_ascii_case(args[0]))
                .ok_or_else(invalid)?;
            if schema
                .fields
                .iter()
                .any(|f| f.name.eq_ignore_ascii_case(args[0]))
            {
                return Err(invalid());
            }
            let mut kind = 0_u32;
            for token in args[1].split('|').map(str::trim) {
                kind |= match token {
                    "DT_BYTE" => 1,
                    "DT_SHORT" => 2,
                    "DT_FLOAT" => 4,
                    "DT_INTEGER" => 8,
                    "DT_ANGLE" => 16,
                    "DT_TIMEWINDOW_8" => 32,
                    "DT_TIMEWINDOW_BIG" => 64,
                    "DT_STRING" => 128,
                    "DT_SIGNED" => 0x8000_0000,
                    _ => return Err(invalid()),
                };
            }
            let pre: f32 = args[3].parse().map_err(|_| invalid())?;
            let bits: u8 = args[2].parse().map_err(|_| invalid())?;
            if !pre.is_finite()
                || !post.is_finite()
                || pre < 0.0
                || post < 0.0
                || f64::from(pre) * 4000.0 > f64::from(u32::MAX)
                || f64::from(post) * 4000.0 > f64::from(u32::MAX)
                || bits == 0
                || bits > 32
                || (kind & 0x7fff_ffff).count_ones() != 1
                || schema.fields.len() >= 64
            {
                return Err(invalid());
            }
            schema.fields.push(Field {
                name: args[0].to_owned(),
                kind,
                offset: reference.offset,
                size: 1,
                bits,
                pre,
                post,
            });
        }
    }
    if current.is_some() || in_body || found.len() != template.len() {
        return Err(invalid());
    }
    let mut ordered = Vec::with_capacity(template.len());
    for reference in &template {
        let schema = found
            .iter()
            .find(|s| s.name.eq_ignore_ascii_case(&reference.name))
            .ok_or_else(invalid)?;
        if schema.fields.is_empty() {
            return Err(invalid());
        }
        ordered.push(schema.clone());
    }
    let fields = ordered.iter().map(|s| s.fields.len()).sum();
    Ok(PreparedDelta {
        bytes: encode(&ordered)?,
        schemas: ordered.len(),
        fields,
        callbacks,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_input_and_truncation() {
        assert!(parse("").is_err());
        assert!(parse("schema\tx\t1\n").is_err());
        assert!(parse("schema\tx\t65\n").is_err());
        assert!(parse("schema\tx\t1\nfield\tx\t8\t0\t1\t8\tNaN\t1\n").is_err());
    }

    #[test]
    fn writes_bits_lsb_first_across_bytes() {
        let mut w = Writer {
            data: Vec::new(),
            bit: 0,
        };
        w.bits(5, 3).unwrap();
        w.bits(0xaa, 8).unwrap();
        assert_eq!(w.data, [0x55, 0x05]);
    }

    #[test]
    #[ignore = "requires separately acquired local HLDS field and binary captures"]
    fn matches_local_hlds_capture() {
        let root = std::env::var_os("GOLDSRC_DELTA_REFERENCE_DIR")
            .expect("set GOLDSRC_DELTA_REFERENCE_DIR to the local fixture directory");
        let root = std::path::Path::new(&root);
        let fields = fs::read_to_string(root.join("hlds-delta-fields-build10211.tsv")).unwrap();
        let expected = fs::read(root.join("hlds-delta-build10211.bin")).unwrap();
        let schemas = parse(&fields).unwrap();
        assert_eq!(schemas.len(), 7);
        assert_eq!(schemas.iter().map(|s| s.fields.len()).sum::<usize>(), 213);
        assert_eq!(encode(&schemas).unwrap(), expected);
    }

    #[test]
    fn prepare_uses_list_values_and_rejects_missing_or_bad_fields() {
        let catalog = "schema\ttest_t\t1\nfield\tx\t8\t4\t1\t8\t1\t1\n";
        let list = "test_t none\n{\nDEFINE_DELTA(x, DT_INTEGER, 8, 1.0)\n}\n";
        let first = prepare(list, catalog).unwrap();
        assert_eq!(first.schemas, 1);
        assert_eq!(first.fields, 1);
        assert!(first.callbacks.is_empty());
        let changed = prepare(&list.replace("8, 1.0", "7, 1.0"), catalog).unwrap();
        assert_ne!(first.bytes, changed.bytes);
        assert!(prepare(&list.replace("DELTA(x", "DELTA(y"), catalog).is_err());
        assert!(prepare(&list.replace("DT_INTEGER", "DT_BAD"), catalog).is_err());
        assert!(prepare(&list.replace("8, 1.0", "33, 1.0"), catalog).is_err());
        assert!(prepare(&list.replace("}\n", ""), catalog).is_err());
    }

    #[test]
    #[ignore = "requires local HLDS installation and separately captured reference"]
    fn generated_list_matches_local_hlds() {
        let root =
            std::env::var_os("GOLDSRC_DELTA_REFERENCE_DIR").expect("missing fixture directory");
        let root = Path::new(&root);
        let prepared = load(
            &root.join("reference-hlds/cstrike/delta.lst"),
            &root.join("hlds-delta-fields-build10211.tsv"),
        )
        .unwrap();
        let expected = fs::read(root.join("hlds-delta-build10211.bin")).unwrap();
        assert_eq!(
            (prepared.schemas, prepared.fields, prepared.callbacks.len()),
            (7, 213, 3)
        );
        assert_eq!(prepared.bytes, expected);
    }
}
